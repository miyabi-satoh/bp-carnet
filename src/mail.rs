//! メール送信 (SMTP、docs/authentication.md)。
//!
//! 設定は必須 (→ `Config::check_required`)。開発・テストでは手元の SMTP サーバーで受ける。

use std::sync::{Arc, Mutex};

use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::Tls;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::{MailConfig, SmtpTls};

/// 1回限りのリンク (確認・再設定) の下に添える、リンクを開けないときの案内。
/// 長い URL はメールアプリによって折り返され、押せなくなることがあるため。
pub const LINK_HELP: &str =
    "リンクを開けない場合は、URL をコピーしてブラウザのアドレス欄に貼り付けてください。\n";

/// SMTP の認証に使うパスワードの環境変数。機微情報のため config.toml には置かない。
pub const PASSWORD_ENV: &str = "SMTP_PASSWORD";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("[mail] from をメールアドレスとして解釈できません: {0}")]
    InvalidFrom(String, #[source] lettre::address::AddressError),
    #[error("[mail] username を設定したときは、環境変数 {PASSWORD_ENV} も設定してください")]
    MissingPassword,
    #[error("SMTP の接続設定を組み立てられません")]
    Configure(#[source] lettre::transport::smtp::Error),
    #[error("送信先をメールアドレスとして解釈できません")]
    InvalidRecipient(#[source] lettre::address::AddressError),
    #[error("メールを組み立てられません")]
    Build(#[source] lettre::error::Error),
    #[error("メールを送信できません")]
    Send(#[source] lettre::transport::smtp::Error),
}

/// [`Mailer::recording`] が送る代わりに記録したメール。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMail {
    pub to: String,
    pub subject: String,
    pub body: String,
}

enum Transport {
    Smtp(AsyncSmtpTransport<Tokio1Executor>),
    Recording(Arc<Mutex<Vec<SentMail>>>),
}

pub struct Mailer {
    transport: Transport,
    from: Mailbox,
    /// 末尾スラッシュを除いた外部公開URL。
    public_url: String,
}

impl Mailer {
    /// `[mail]` と環境変数 `SMTP_PASSWORD` から組み立てる。`username` があるのに `SMTP_PASSWORD` が無ければエラーにする。
    ///
    /// Tokio のランタイムの中で呼ぶこと。lettre の接続プールが組み立て時にタスクを spawn する。
    pub fn from_config(config: &MailConfig, public_url: &str) -> Result<Self, Error> {
        let password = crate::config::non_empty_env(PASSWORD_ENV);
        Self::build(config, public_url, password)
    }

    fn build(
        config: &MailConfig,
        public_url: &str,
        password: Option<String>,
    ) -> Result<Self, Error> {
        let public_url = public_url.trim_end_matches('/');
        let from = config
            .from
            .parse::<Mailbox>()
            .map_err(|err| Error::InvalidFrom(config.from.clone(), err))?;

        let builder = match config.tls {
            SmtpTls::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                .map_err(Error::Configure)?,
            SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
                .map_err(Error::Configure)?,
            SmtpTls::None => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host).tls(Tls::None)
            }
        };
        let mut builder = builder.port(config.port);
        if !config.username.is_empty() {
            let password = password.ok_or(Error::MissingPassword)?;
            builder = builder.credentials(Credentials::new(config.username.clone(), password));
        }

        Ok(Self {
            transport: Transport::Smtp(builder.build()),
            from,
            public_url: public_url.to_string(),
        })
    }

    /// 送らずに記録する `Mailer`。統合テストで送った内容を確かめるために使う。
    pub fn recording(public_url: &str) -> (Self, Arc<Mutex<Vec<SentMail>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mailer = Self {
            transport: Transport::Recording(Arc::clone(&sent)),
            from: "BP Carnet <noreply@example.com>"
                .parse()
                .expect("固定の送信元は解釈できるはず"),
            public_url: public_url.trim_end_matches('/').to_string(),
        };
        (mailer, sent)
    }

    /// 送信先をメールアドレスとして解釈できる (管理者が作ったユーザーIDはメールアドレスとは限らない)。
    pub fn can_send_to(&self, address: &str) -> bool {
        address.parse::<lettre::Address>().is_ok()
    }

    /// メールに載せるリンク。`path` は `/` で始めること。
    pub fn link(&self, path: &str) -> String {
        format!("{}{path}", self.public_url)
    }

    /// 本文がプレーンテキストのメールを1通送る。本文の後ろに共通の署名 ([`signature`]) を付ける。
    pub async fn send(&self, to: &str, subject: &str, body: String) -> Result<(), Error> {
        let recipient = to.parse::<Mailbox>().map_err(Error::InvalidRecipient)?;
        let body = format!("{}\n\n{}", body.trim_end(), signature(&self.public_url));

        match &self.transport {
            Transport::Smtp(transport) => {
                let message = Message::builder()
                    .from(self.from.clone())
                    .to(recipient)
                    .subject(subject)
                    .header(ContentType::TEXT_PLAIN)
                    .body(body)
                    .map_err(Error::Build)?;
                transport.send(message).await.map_err(Error::Send)?;
            }
            Transport::Recording(sent) => {
                sent.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(SentMail {
                        to: recipient.email.to_string(),
                        subject: subject.to_string(),
                        body,
                    });
            }
        }
        Ok(())
    }
}

/// すべてのメールの末尾に付ける署名。どのサイトから来たメールかが分かるよう、サイトの URL を載せる。
///
/// ADR: 「送信専用」とは書かない。送信元 (`[mail] from`) は運営者が決め、管理者本人の Gmail で
/// 送る構成では返信が届くため。
fn signature(public_url: &str) -> String {
    format!(
        "――\n\
         このメールは BP Carnet から自動で送信しています。\n\
         BP Carnet がメールや電話でパスワードを尋ねることはありません。\n\
         BP Carnet {public_url}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(host: &str, from: &str, username: &str) -> MailConfig {
        MailConfig {
            host: host.to_string(),
            from: from.to_string(),
            username: username.to_string(),
            ..MailConfig::default()
        }
    }

    #[tokio::test]
    async fn every_setting_builds_the_mailer() {
        let mailer = Mailer::build(
            &config(
                "smtp.example.com",
                "BP Carnet <noreply@example.com>",
                "user",
            ),
            "https://bp.example.com/",
            Some("secret".to_string()),
        )
        .expect("組み立てられるはず");
        assert_eq!(
            mailer.link("/reset-password?token=abc"),
            "https://bp.example.com/reset-password?token=abc"
        );
    }

    #[test]
    fn invalid_from_is_rejected() {
        let result = Mailer::build(
            &config("smtp.example.com", "not an address", ""),
            "https://x",
            None,
        );
        assert!(matches!(result, Err(Error::InvalidFrom(..))));
    }

    #[test]
    fn username_without_password_is_rejected() {
        let result = Mailer::build(
            &config("smtp.example.com", "noreply@example.com", "user"),
            "https://x",
            None,
        );
        assert!(matches!(result, Err(Error::MissingPassword)));
    }

    #[tokio::test]
    async fn recording_mailer_keeps_sent_mail_with_the_signature() {
        let (mailer, sent) = Mailer::recording("https://bp.example.com");
        mailer
            .send("kyoko@example.com", "件名", "本文".to_string())
            .await
            .expect("記録できるはず");
        assert_eq!(
            *sent.lock().expect("ロックできるはず"),
            vec![SentMail {
                to: "kyoko@example.com".to_string(),
                subject: "件名".to_string(),
                body: format!("本文\n\n{}", signature("https://bp.example.com")),
            }]
        );
        let body = &sent.lock().expect("ロックできるはず")[0].body;
        assert!(body.contains("https://bp.example.com"), "{body}");
    }
}
