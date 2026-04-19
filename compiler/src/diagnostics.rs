use std::fmt::Write;

use crate::source::{SourceDb, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span,
        }
    }

    pub fn warning(code: &'static str, message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span,
        }
    }
}

#[derive(Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn error(&mut self, code: &'static str, message: impl Into<String>, span: Option<Span>) {
        self.push(Diagnostic::error(code, message, span));
    }

    pub fn warning(&mut self, code: &'static str, message: impl Into<String>, span: Option<Span>) {
        self.push(Diagnostic::warning(code, message, span));
    }

    pub fn extend(&mut self, mut other: Diagnostics) {
        self.items.append(&mut other.items);
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn sort_deterministically(&mut self) {
        self.items.sort_by_key(|d| {
            (
                d.span.map(|s| s.file_id).unwrap_or(u32::MAX),
                d.span.map(|s| s.start).unwrap_or(usize::MAX),
                d.code,
            )
        });
    }

    pub fn render(&self, source_db: &SourceDb) -> String {
        let mut output = String::new();

        for diagnostic in &self.items {
            let sev = match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };

            if let Some(span) = diagnostic.span {
                let file = source_db.file(span.file_id);
                let (line, col) = file.line_col(span.start);
                let _ = writeln!(
                    output,
                    "{sev}[{}]: {}:{}:{}: {}",
                    diagnostic.code,
                    file.path.display(),
                    line,
                    col,
                    diagnostic.message
                );

                if let Some(src_line) = file.text.lines().nth(line.saturating_sub(1)) {
                    let _ = writeln!(output, "    {src_line}");
                    let mut marker = String::new();
                    for _ in 1..col {
                        marker.push(' ');
                    }
                    marker.push('^');
                    let _ = writeln!(output, "    {marker}");
                }
            } else {
                let _ = writeln!(output, "{sev}[{}]: {}", diagnostic.code, diagnostic.message);
            }
        }

        output
    }

    pub fn into_items(self) -> Vec<Diagnostic> {
        self.items
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }
}
