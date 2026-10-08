//! SvelteKit (SPA) のビルド成果物の配信。

use axum::{
    extract::State,
    http::{HeaderName, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

use crate::seo;
use crate::state::AppState;

/// `frontend/build/` は SvelteKit (adapter-static, SPA fallback: `index.html`) の
/// ビルド成果物を配置する場所。release ビルド時にバイナリへ埋め込まれる
/// (debug ビルドではディスクから直接読む)。
///
/// ADR: `.woff` を除く。fontsource の CSS は woff2 と woff の両方を参照するが、woff は
/// woff2 非対応ブラウザ向けの互換ファイルで、現行ブラウザは読まない。日本語フォント
/// (M PLUS Rounded 1c) は1ウェイトあたり woff2 だけで約2.7MBあり、両形式を埋め込むと
/// 実行ファイルが倍近く膨らむため、実際に使われる woff2 のみを埋め込む。
#[derive(RustEmbed)]
#[folder = "frontend/build/"]
#[exclude = "*.woff"]
struct Assets;

const INDEX_HTML: &str = "index.html";

const X_ROBOTS_TAG: HeaderName = HeaderName::from_static("x-robots-tag");

/// API 以外の全リクエストを受けるフォールバックハンドラ。
/// - 埋め込みアセットに一致するパスがあればそれを返す
/// - 一致しなければ SPA のエントリーポイントとして index.html を返す
///
/// index.html には、パスに応じた検索エンジン向けのタグを差し込む (`seo`)。
/// 存在しないパスも 200 で index.html を返すため、noindex で索引から外す。
pub async fn handler(State(state): State<AppState>, uri: Uri) -> Response {
    let request_path = uri.path();
    let path = request_path.trim_start_matches('/');
    let path = if path.is_empty() { INDEX_HTML } else { path };

    // `/index.html` の直接の要求は、SPA の枠としてほかのパスと同じに扱う (索引の対象は `/`)。
    match Assets::get(path).filter(|_| path != INDEX_HTML) {
        Some(file) => serve(path, file),
        None => match Assets::get(INDEX_HTML) {
            Some(file) => serve_index(&state.public_url, request_path, file),
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

pub async fn sitemap(State(state): State<AppState>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/xml"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        seo::sitemap(&state.public_url),
    )
        .into_response()
}

pub async fn robots(State(state): State<AppState>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        seo::robots(&state.public_url),
    )
        .into_response()
}

/// index.html を、`request_path` 向けのタグを `</head>` の前に差し込んで返す。
/// 毎回の文字列の組み立ては 20KB 足らずで、キャッシュするほどの費用ではない。
fn serve_index(public_url: &str, request_path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let Ok(html) = std::str::from_utf8(&file.data) else {
        return serve(INDEX_HTML, file);
    };
    let body = seo::inject(html, &seo::head_tags(public_url, request_path));
    let mut response = (
        [
            (header::CONTENT_TYPE, "text/html".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        body,
    )
        .into_response();
    if seo::is_noindex(request_path) {
        response
            .headers_mut()
            .insert(X_ROBOTS_TAG, "noindex".parse().expect("固定の値"));
    }
    response
}

fn serve(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache_control = if path.starts_with("_app/immutable/") {
        // content-hash 付きファイルは恒久キャッシュ可能
        "public, max-age=31536000, immutable"
    } else {
        // index.html など、更新をすぐ反映したいもの
        "no-cache"
    };

    (
        [
            (header::CONTENT_TYPE, mime.as_ref().to_string()),
            (header::CACHE_CONTROL, cache_control.to_string()),
        ],
        file.data,
    )
        .into_response()
}
