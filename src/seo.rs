//! 検索エンジン向けの情報 (docs/architecture.md)。
//! index.html への差し込み・sitemap.xml・robots.txt の中身を作る。

/// 索引させるページの title・description。
struct Page {
    path: String,
    title: String,
    description: String,
}

const SITE_NAME: &str = "BP Carnet";

/// 使い方 (`/help/<slug>`) の項目。並びは `frontend/src/lib/help.ts` の `HELP_TOPICS` に合わせ、
/// title は `frontend/messages/ja.json` の `help_<slug>_title` と同じにする。
/// Rust から読めないため二重に持つ。食い違いはテスト (`help_topics_match_the_frontend`) が拾う。
const HELP_TOPICS: [(&str, &str); 21] = [
    ("login", "ログインする"),
    ("signup", "メールアドレスでアカウントを作る"),
    ("record-by-hand", "手動で入力する"),
    ("record-by-photo", "写真で記録する"),
    ("import-notebook", "手帳の記録をまとめて取り込む"),
    ("edit-record", "記録を直す・消す"),
    ("view-records", "記録を見る"),
    ("print-report", "印刷する"),
    ("settings-period", "朝と夜の時間帯を変える"),
    ("settings-timezone", "タイムゾーンを変える"),
    ("theme", "画面の明るさを変える"),
    ("export-csv", "記録を書き出す"),
    ("import-csv", "CSV ファイルから記録を取り込む"),
    ("delete-account", "アカウントを削除する"),
    ("trouble-login", "ログインできないとき"),
    (
        "trouble-in-app-browser",
        "LINE のリンクなどから開いてログインできないとき",
    ),
    (
        "trouble-line-email",
        "LINE でメールアドレスを受け取れないと言われたとき",
    ),
    ("trouble-photo", "写真が読み取れないとき"),
    ("trouble-ocr-budget", "無料の読み取りを使い切ったとき"),
    ("home-screen", "iPhone のアプリを入れるには"),
    ("home-screen-android", "ホーム画面に追加するには (Android)"),
];

/// 索引させるページの一覧。ここに無いパス (ログインが要る画面・`/login` など・存在しないパス) は noindex。
/// `/login` は、未ログインの `/` がログイン画面に見えるのと重複するため索引させない。
fn pages() -> Vec<Page> {
    let mut pages = vec![
        Page {
            path: "/".into(),
            title: "BP Carnet — 血圧の記録を、写真を撮るだけで".into(),
            description: "血圧計の画面や手書きの記録を写真に撮るだけで、日付と数値を読み取って記録できる血圧記録アプリ。朝と夜の推移をグラフで見られ、通院のときに見せる用に A4 で印刷もできます。".into(),
        },
        Page {
            path: "/terms".into(),
            title: format!("利用規約 — {SITE_NAME}"),
            description: "血圧記録アプリ BP Carnet の利用規約です。サービスの利用条件、禁止事項、免責、退会したときの扱いなどを定めています。".into(),
        },
        Page {
            path: "/privacy".into(),
            title: format!("プライバシーポリシー — {SITE_NAME}"),
            description: "BP Carnet が取り扱う情報の種類、利用の目的、保管と第三者への提供、削除の方法をまとめたプライバシーポリシーです。".into(),
        },
        Page {
            path: "/pricing".into(),
            title: format!("料金と購入の条件 — {SITE_NAME}"),
            description: "BP Carnet で写真の読み取りを買い足すときの価格と、使える環境です。".into(),
        },
        Page {
            path: "/help".into(),
            title: format!("使い方 — {SITE_NAME}"),
            description: "BP Carnet の使い方の案内です。ログイン、写真や手入力での記録、グラフの見かた、印刷、設定、困ったときの対処を、手順と画面の画像で説明します。".into(),
        },
    ];
    pages.extend(HELP_TOPICS.iter().map(|(slug, title)| Page {
        path: format!("/help/{slug}"),
        title: format!("{title} — 使い方 — {SITE_NAME}"),
        description: format!("BP Carnet の使い方「{title}」を、手順と画面の画像で説明します。"),
    }));
    pages
}

/// sitemap.xml の中身。索引させるページだけを絶対 URL で並べる。`public_url` は末尾の `/` を除いたもの。
pub(crate) fn sitemap(public_url: &str) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for page in pages() {
        let loc = escape(&absolute_url(public_url, &page.path));
        xml.push_str(&format!("  <url><loc>{loc}</loc></url>\n"));
    }
    xml.push_str("</urlset>\n");
    xml
}

/// robots.txt。全許可のまま Sitemap の場所を伝える。
/// `/api/` は塞がない: JSON を返すだけで索引の対象にならず、塞ぐと noindex の指示 (`X-Robots-Tag`) が読まれなくなる。
pub(crate) fn robots(public_url: &str) -> String {
    format!("User-agent: *\nDisallow:\n\nSitemap: {public_url}/sitemap.xml\n")
}

/// `<head>` に差し込むタグ。
/// - 索引させるページなら、title・description・canonical・OGP
/// - それ以外は noindex
pub(crate) fn head_tags(public_url: &str, path: &str) -> String {
    match indexable(path) {
        Some(page) => {
            let title = escape(&page.title);
            let description = escape(&page.description);
            let url = escape(&absolute_url(public_url, path));
            let site = escape(SITE_NAME);
            format!(
                "<title>{title}</title>\n\
                 <meta name=\"description\" content=\"{description}\" />\n\
                 <link rel=\"canonical\" href=\"{url}\" />\n\
                 <meta property=\"og:title\" content=\"{title}\" />\n\
                 <meta property=\"og:description\" content=\"{description}\" />\n\
                 <meta property=\"og:type\" content=\"website\" />\n\
                 <meta property=\"og:url\" content=\"{url}\" />\n\
                 <meta property=\"og:site_name\" content=\"{site}\" />\n\
                 <meta property=\"og:locale\" content=\"ja_JP\" />\n\
                 <meta name=\"twitter:card\" content=\"summary\" />\n"
            )
        }
        None => "<meta name=\"robots\" content=\"noindex\" />\n".to_string(),
    }
}

/// `X-Robots-Tag: noindex` を付けるか。HTML の meta を読まない取得にも効かせる。
pub(crate) fn is_noindex(path: &str) -> bool {
    indexable(path).is_none()
}

/// index.html の `</head>` の直前へ `tags` を差し込む。`</head>` が見つからなければそのまま返す
/// (差し込めなくても画面は動くため)。
pub(crate) fn inject(html: &str, tags: &str) -> String {
    match html.find("</head>") {
        Some(at) => format!("{}{tags}{}", &html[..at], &html[at..]),
        None => html.to_string(),
    }
}

fn indexable(path: &str) -> Option<Page> {
    pages().into_iter().find(|page| page.path == path)
}

fn absolute_url(public_url: &str, path: &str) -> String {
    if path == "/" {
        format!("{public_url}/")
    } else {
        format!("{public_url}{path}")
    }
}

/// HTML の属性値・テキストと XML の文字列に使える最小のエスケープ。
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://bp.example.com";

    #[test]
    fn escape_covers_markup_characters() {
        assert_eq!(escape(r#"a&b<c>"d'"#), "a&amp;b&lt;c&gt;&quot;d&#39;");
    }

    #[test]
    fn public_page_gets_canonical_and_ogp() {
        let tags = head_tags(URL, "/terms");
        assert!(tags.contains(r#"<link rel="canonical" href="https://bp.example.com/terms" />"#));
        assert!(tags.contains(r#"property="og:url" content="https://bp.example.com/terms""#));
        assert!(!tags.contains("noindex"));
    }

    #[test]
    fn top_canonical_ends_with_slash() {
        assert!(head_tags(URL, "/").contains(r#"href="https://bp.example.com/""#));
    }

    #[test]
    fn every_public_page_gets_title_description_and_ogp() {
        for path in [
            "/",
            "/terms",
            "/privacy",
            "/pricing",
            "/help",
            "/help/login",
        ] {
            let tags = head_tags(URL, path);
            assert!(!is_noindex(path), "{path}");
            for needle in [
                "<title>",
                r#"<meta name="description" content=""#,
                r#"property="og:title""#,
                r#"property="og:description""#,
                r#"<meta property="og:type" content="website" />"#,
                r#"property="og:site_name""#,
                r#"<meta property="og:locale" content="ja_JP" />"#,
                r#"<meta name="twitter:card" content="summary" />"#,
            ] {
                assert!(tags.contains(needle), "{path}: {needle}");
            }
            assert!(!tags.contains("og:image"), "{path}");
            assert!(!tags.contains("noindex"), "{path}");
        }
    }

    #[test]
    fn sitemap_lists_only_public_pages_with_absolute_urls() {
        let xml = sitemap(URL);
        let locs: Vec<&str> = xml
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("<url><loc>")?
                    .strip_suffix("</loc></url>")
            })
            .collect();
        assert!(locs.starts_with(&[
            "https://bp.example.com/",
            "https://bp.example.com/terms",
            "https://bp.example.com/privacy",
            "https://bp.example.com/pricing",
            "https://bp.example.com/help",
            "https://bp.example.com/help/login",
        ]));
        assert!(
            locs.iter()
                .all(|loc| loc.starts_with("https://bp.example.com/"))
        );
        assert!(!locs.contains(&"https://bp.example.com/login"));
        assert!(!locs.contains(&"https://bp.example.com/settings"));
    }

    #[test]
    fn robots_allows_everything_and_points_at_the_sitemap() {
        let body = robots(URL);
        assert!(body.contains("Disallow:\n"));
        assert!(!body.contains("/api"));
        assert!(body.contains("Sitemap: https://bp.example.com/sitemap.xml"));
    }

    #[test]
    fn unknown_and_private_paths_are_noindex() {
        for path in [
            "/login",
            "/signup",
            "/settings",
            "/admin/users",
            "/nope",
            "/help/",
            "/help/nope",
            "/terms/",
            "/index.html",
        ] {
            assert!(is_noindex(path), "{path}");
            assert!(head_tags(URL, path).contains("noindex"), "{path}");
        }
    }

    #[test]
    fn injected_values_are_escaped() {
        let tags = head_tags("https://a.example.com/?x=\"<&", "/");
        assert!(!tags.contains("\"<&"));
        assert!(tags.contains("&quot;&lt;&amp;"));
    }

    #[test]
    fn inject_goes_before_head_end() {
        assert_eq!(inject("<head><b></head>x", "T"), "<head><b>T</head>x");
        assert_eq!(inject("no head", "T"), "no head");
    }

    /// `frontend/src/lib/help.ts` の slug と `messages/ja.json` の title が、`HELP_TOPICS` と一致する。
    #[test]
    fn help_topics_match_the_frontend() {
        let root = env!("CARGO_MANIFEST_DIR");
        let ts = std::fs::read_to_string(format!("{root}/frontend/src/lib/help.ts"))
            .expect("help.ts を読めること");
        let slugs: Vec<&str> = ts
            .lines()
            .filter_map(|line| line.trim().strip_prefix("slug: '")?.strip_suffix("',"))
            .collect();
        let ours: Vec<&str> = HELP_TOPICS.iter().map(|(slug, _)| *slug).collect();
        assert_eq!(slugs, ours);

        let ja: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/frontend/messages/ja.json"))
                .expect("ja.json を読めること"),
        )
        .expect("ja.json は JSON");
        for (slug, title) in HELP_TOPICS {
            let key = format!("help_{}_title", slug.replace('-', "_"));
            assert_eq!(ja[&key].as_str(), Some(title), "{key}");
        }
    }

    /// 固定ページと使い方の title が、フロント (`frontend/src/lib/page-title.ts`) が出す文言と一致する。
    #[test]
    fn page_titles_match_the_frontend() {
        let root = env!("CARGO_MANIFEST_DIR");
        let ja: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/frontend/messages/ja.json"))
                .expect("ja.json を読めること"),
        )
        .expect("ja.json は JSON");
        let msg = |key: &str| {
            ja[key]
                .as_str()
                .unwrap_or_else(|| panic!("{key}"))
                .to_string()
        };
        let app = msg("app_name");
        let help = msg("help_title");

        let mut expected = vec![
            ("/".to_string(), msg("seo_top_title")),
            (
                "/terms".to_string(),
                format!("{} — {app}", msg("terms_title")),
            ),
            (
                "/privacy".to_string(),
                format!("{} — {app}", msg("privacy_title")),
            ),
            (
                "/pricing".to_string(),
                format!("{} — {app}", msg("pricing_title")),
            ),
            ("/help".to_string(), format!("{help} — {app}")),
        ];
        for (slug, _) in HELP_TOPICS {
            let title = msg(&format!("help_{}_title", slug.replace('-', "_")));
            expected.push((format!("/help/{slug}"), format!("{title} — {help} — {app}")));
        }
        let actual: Vec<(String, String)> = pages()
            .into_iter()
            .map(|page| (page.path, page.title))
            .collect();
        assert_eq!(actual, expected);
    }
}
