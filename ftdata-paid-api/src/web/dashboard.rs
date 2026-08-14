//! Home page handler `GET /dashboard/`.

use axum::{
    extract::State,
    response::Html,
};
use maud::{html, Markup};

use crate::state::AppState;

/// Render the base HTML shell used by all dashboard pages.
pub(crate) fn shell(title: &str, body: Markup) -> Markup {
    html! {
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                style {
                    "body { font-family: -apple-system, sans-serif; max-width: 900px; margin: 0 auto; padding: 2rem; background: #f9f9f9; }"
                    "h1, h2 { color: #333; }"
                    "nav { margin: 1.5rem 0; padding: 1rem; background: #fff; border-radius: 6px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }"
                    "nav a { margin-right: 1.5rem; color: #0066cc; text-decoration: none; }"
                    "nav a:hover { text-decoration: underline; }"
                    "table { width: 100%; border-collapse: collapse; background: #fff; border-radius: 6px; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }"
                    "th, td { padding: 0.75rem 1rem; text-align: left; border-bottom: 1px solid #eee; }"
                    "th { background: #f0f0f0; font-weight: 600; }"
                    "tr:last-child td { border-bottom: none; }"
                    ".card { background: #fff; padding: 1.5rem; border-radius: 6px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); margin-bottom: 1rem; }"
                    ".stat { font-size: 2rem; font-weight: 700; color: #0066cc; }"
                    ".stat-label { color: #666; font-size: 0.9rem; }"
                    ".stats-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; margin-bottom: 1.5rem; }"
                    ".badge { display: inline-block; padding: 0.2rem 0.6rem; border-radius: 4px; font-size: 0.85rem; font-weight: 600; }"
                    ".badge-queued { background: #eaf4ff; color: #0066cc; }"
                    ".badge-running { background: #fff8e6; color: #cc8800; }"
                    ".badge-completed { background: #e6f9e6; color: #009900; }"
                    ".badge-failed { background: #ffebeb; color: #cc0000; }"
                    ".badge-cancelled { background: #f0f0f0; color: #666; }"
                    "a { color: #0066cc; }"
                    "form { background: #fff; padding: 1.5rem; border-radius: 6px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }"
                    "label { display: block; margin: 0.75rem 0 0.25rem; font-weight: 600; color: #333; }"
                    "input[type=text], select, textarea { width: 100%; padding: 0.5rem; border: 1px solid #ccc; border-radius: 4px; box-sizing: border-box; }"
                    "textarea { height: 100px; }"
                    "button { margin-top: 1rem; padding: 0.6rem 1.5rem; background: #0066cc; color: #fff; border: none; border-radius: 4px; cursor: pointer; font-size: 1rem; }"
                    "button:hover { background: #0055aa; }"
                    ".checkbox-group { display: flex; gap: 1rem; flex-wrap: wrap; margin-top: 0.25rem; }"
                    ".checkbox-group label { font-weight: normal; margin: 0; }"
                    ".info-row { display: flex; justify-content: space-between; padding: 0.5rem 0; border-bottom: 1px solid #eee; }"
                    ".info-row:last-child { border-bottom: none; }"
                }
            }
            body {
                nav {
                    a href="/dashboard/" { "Home" }
                    a href="/dashboard/quote" { "New Quote" }
                    a href="/dashboard/jobs" { "Jobs" }
                }
                (body)
            }
        }
    }
}

pub async fn home(State(state): State<AppState>) -> Html<String> {
    let total_jobs = state.jobs.len() as u64;
    let completed_jobs = state
        .jobs
        .iter_rev()
        .filter(|j| j.status == crate::jobs::JobStatus::Completed)
        .count() as u64;

    // Sum revenue from receipts
    let revenue_total: f64 = state
        .receipts
        .iter()
        .filter_map(|r| r.amount_usdc.parse::<f64>().ok())
        .sum();

    let recent_jobs: Vec<_> = state
        .jobs
        .iter_rev()
        .take(5)
        .collect();

    let markup = shell("ftdata-paid Dashboard", html! {
        h1 { "ftdata-paid API" }
        p { "Service is running. Version 1.0.0" }

        .card {
            h2 { "Quick Stats" }
            .stats-grid {
                .card {
                    .stat { (total_jobs) }
                    .stat-label { "Total Jobs" }
                }
                .card {
                    .stat { (completed_jobs) }
                    .stat-label { "Completed" }
                }
                .card {
                    .stat { (format!("{:.4}", revenue_total)) }
                    .stat-label { "Revenue (USDC)" }
                }
            }
        }

        .card {
            h2 { "Quick Links" }
            p {
                a href="/dashboard/quote" { "Request a Quote" } " — get a price estimate for your data request"
            }
            p {
                a href="/dashboard/jobs" { "View All Jobs" } " — see job history and status"
            }
        }

        .card {
            h2 { "Recent Jobs" }
            @if recent_jobs.is_empty() {
                p { "No jobs yet." }
            } @else {
                table {
                    thead {
                        tr {
                            th { "ID" }
                            th { "Status" }
                            th { "Amount Paid" }
                        }
                    }
                    tbody {
                        @for job in recent_jobs {
                            tr {
                                td {
                                    a href=(format!("/dashboard/jobs/{}", job.id)) {
                                        (job.id.chars().take(12).collect::<String>())
                                    }
                                }
                                td {
                                    span class=(format!("badge badge-{}", serde_json::to_string(&job.status).unwrap_or_default().trim_matches('"'))) {
                                        (serde_json::to_string(&job.status).unwrap_or_default().trim_matches('"'))
                                    }
                                }
                                td { (job.amount_paid_usdc) }
                            }
                        }
                    }
                }
            }
        }
    });

    Html(markup.into_string())
}
