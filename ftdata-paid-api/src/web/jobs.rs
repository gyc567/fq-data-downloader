//! Jobs list + detail pages.

use axum::{
    extract::{Path, State},
    response::Html,
};
use maud::html;

use crate::jobs::JobStatus;
use crate::state::AppState;

use super::dashboard::shell;

fn status_badge(status: &JobStatus) -> maud::Markup {
    let label = match status {
        JobStatus::Queued => "Queued",
        JobStatus::Running => "Running",
        JobStatus::Completed => "Completed",
        JobStatus::Failed => "Failed",
        JobStatus::Cancelled => "Cancelled",
    };
    let cls = match status {
        JobStatus::Queued => "badge-queued",
        JobStatus::Running => "badge-running",
        JobStatus::Completed => "badge-completed",
        JobStatus::Failed => "badge-failed",
        JobStatus::Cancelled => "badge-cancelled",
    };
    html! { span class=(format!("badge {}", cls)) { (label) } }
}

pub async fn list(State(state): State<AppState>) -> Html<String> {
    let jobs: Vec<_> = state.jobs.iter_rev().collect();

    let markup = shell("Jobs — ftdata-paid", html! {
        h1 { "Jobs" }

        .card {
            p { (jobs.len()) " total job(s)" }
        }

        @if jobs.is_empty() {
            .card {
                p { "No jobs yet. Submit a quote and download request to get started." }
                p {
                    a href="/dashboard/quote" { "Request a Quote" }
                }
            }
        } @else {
            table {
                thead {
                    tr {
                        th { "ID" }
                        th { "Status" }
                        th { "Progress" }
                        th { "Amount Paid" }
                        th { "TX Hash" }
                        th { "Quote ID" }
                    }
                }
                tbody {
                    @for job in jobs {
                        tr {
                            td {
                                a href=(format!("/dashboard/jobs/{}", job.id)) {
                                    (job.id.chars().take(16).collect::<String>())
                                }
                            }
                            td { (status_badge(&job.status)) }
                            td { (format!("{:.0}%", job.progress * 100.0)) }
                            td { (job.amount_paid_usdc) }
                            td {
                                @if let Some(ref hash) = job.tx_hash {
                                    (hash.chars().take(10).collect::<String>()) "..."
                                } @else {
                                    "—"
                                }
                            }
                            td { (job.quote_id.chars().take(12).collect::<String>()) "..." }
                        }
                    }
                }
            }
        }

        p style="margin-top:1rem;" {
            a href="/dashboard/" { "← Back to Dashboard" }
        }
    });

    Html(markup.into_string())
}

pub async fn detail(Path(id): Path<String>, State(state): State<AppState>) -> Html<String> {
    let job = state.jobs.get(&id);

    let markup = shell("Job Detail — ftdata-paid", html! {
        @if let Some(job) = job {
            .card {
                h2 { "Job " (job.id.chars().take(16).collect::<String>()) }
                (status_badge(&job.status))
            }

            .card {
                h3 { "Details" }
                .info-row {
                    span { "Job ID" }
                    span { (job.id) }
                }
                .info-row {
                    span { "Status" }
                    (status_badge(&job.status))
                }
                .info-row {
                    span { "Progress" }
                    span { (format!("{:.1}%", job.progress * 100.0)) }
                }
                .info-row {
                    span { "Amount Paid" }
                    span { (job.amount_paid_usdc) }
                }
                .info-row {
                    span { "Quote ID" }
                    span { (job.quote_id) }
                }
                .info-row {
                    span { "Payer" }
                    span {
                        @if let Some(ref p) = job.payer {
                            (p)
                        } @else {
                            "—"
                        }
                    }
                }
                .info-row {
                    span { "TX Hash" }
                    span {
                        @if let Some(ref h) = job.tx_hash {
                            (h)
                        } @else {
                            "—"
                        }
                    }
                }
            }

            @if let Some(ref err) = job.error {
                .card {
                    h3 style="color:#cc0000;" { "Error" }
                    pre style="background:#ffebeb;padding:1rem;border-radius:4px;overflow:auto;" { (err) }
                }
            }

            @if let Some(ref result) = job.result {
                .card {
                    h3 { "Result" }
                    p { (result.files.len()) " file(s)" }
                    table {
                        thead {
                            tr {
                                th { "File" }
                                th { "Size (bytes)" }
                                th { "SHA256" }
                                th { "Download URL" }
                            }
                        }
                        tbody {
                            @for file in &result.files {
                                tr {
                                    td { (file.name) }
                                    td { (file.bytes) }
                                    td style="font-family:monospace;font-size:0.85rem;" { (file.sha256.chars().take(16).collect::<String>()) "..." }
                                    td {
                                        a href=(format!("{}&expires={}", file.download_url, file.expires_at)) { "Download" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } @else {
            .card {
                h2 { "Job Not Found" }
                p { "No job found with ID: " (id) }
            }
        }

        p {
            a href="/dashboard/jobs" { "← Back to Jobs" }
            " | "
            a href="/dashboard/" { "Dashboard" }
        }
    });

    Html(markup.into_string())
}
