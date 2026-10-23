use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use crate::AppState;

const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// GET /
pub async fn index(State(state): State<AppState>) -> Html<String> {
    let metadata = state.metadata.read().await;
    let mut items = String::new();

    for (name, project) in &metadata.projects {
        let public_count = project.files.values().filter(|file| file.public).count();
        if public_count == 0 {
            continue;
        }

        let href = format!("/ui/{}", utf8_percent_encode(name, PATH_SEGMENT));
        items.push_str(&format!(
            r#"<li><a href="{href}">{name}</a> <span class="muted">({public_count} public file{plural})</span></li>"#,
            name = escape_html(name),
            plural = if public_count == 1 { "" } else { "s" },
        ));
    }

    if items.is_empty() {
        items = r#"<li class="muted">No public assets yet.</li>"#.to_string();
    }

    let body = format!(r#"<h1>Projects</h1><ul class="projects">{items}</ul>"#);
    Html(page("Assets", &body))
}

/// GET /ui/{project}
pub async fn project(State(state): State<AppState>, Path(project): Path<String>) -> Response {
    let metadata = state.metadata.read().await;
    let Some(project_entry) = metadata.projects.get(&project) else {
        return not_found();
    };

    let project_segment = utf8_percent_encode(&project, PATH_SEGMENT);
    let mut rows = String::new();
    let mut has_public_file = false;

    for (name, file) in &project_entry.files {
        if !file.public {
            continue;
        }
        has_public_file = true;

        let file_segment = utf8_percent_encode(name, PATH_SEGMENT);
        let href = format!("/assets/{project_segment}/{file_segment}");
        rows.push_str(&format!(
            r#"<tr><td><a href="{href}">{name}</a></td><td>{size}</td><td>{modified}</td></tr>"#,
            name = escape_html(name),
            size = human_size(file.size),
            modified = file
                .modified_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ));
    }

    if !has_public_file {
        return not_found();
    }

    let body = format!(
        r#"<p><a href="/">&larr; Projects</a></p><h1>{project}</h1><table><thead><tr><th>File</th><th>Size</th><th>Modified</th></tr></thead><tbody>{rows}</tbody></table>"#,
        project = escape_html(&project),
    );

    Html(page(&project, &body)).into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Html(page(
            "Not Found",
            r#"<h1>Not Found</h1><p><a href="/">&larr; Projects</a></p>"#,
        )),
    )
        .into_response()
}

fn escape_html(value: &str) -> String {
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

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.1} {}", UNITS[unit])
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
body {{ font-family: system-ui, sans-serif; max-width: 960px; margin: 2rem auto; padding: 0 1rem; color: #222; }}
h1 {{ font-size: 1.4rem; }}
a {{ color: #0b5fff; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
ul.projects {{ list-style: none; padding: 0; }}
ul.projects li {{ padding: .4rem 0; border-bottom: 1px solid #eee; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ text-align: left; padding: .4rem .5rem; border-bottom: 1px solid #eee; }}
th {{ font-size: .8rem; text-transform: uppercase; color: #666; }}
.muted {{ color: #777; font-size: .9rem; }}
</style>
</head>
<body>{body}</body>
</html>"#,
        title = escape_html(title),
    )
}
