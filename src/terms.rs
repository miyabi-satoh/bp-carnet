//! 利用規約・プライバシーポリシーへの同意 (docs/data-model.md の `terms_version`)。
//!
//! 本文は frontend に同梱する (`frontend/src/lib/legal/`)。

/// 今の規約の版。本文 (規約・ポリシーのどちらか) の表示される文言を変えたら上げる (リンクの書き方だけなら上げない)。サインアップで同意した版として記録する。
pub const VERSION: &str = "11";
