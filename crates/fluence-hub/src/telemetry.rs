// SPDX-License-Identifier: Apache-2.0

//! Structured logging with **P0 redaction** (PLAN 2.1; SPEC §9.A: no P0
//! data in logs, ever).
//!
//! Two complementary layers (ADR-0005 §4):
//!
//! 1. **Types**: P0 values travel through the hub as
//!    [`secrecy::SecretString`] — no `Display` impl (cannot be
//!    interpolated into a log message), `Debug` prints `[REDACTED]`.
//!    Reaching the content requires an explicit, greppable
//!    `expose_secret()`.
//! 2. **Field denylist** (this module): even if a raw value reaches a
//!    tracing field whose name says "user content"
//!    ([`P0_FIELD_DENYLIST`]), the formatter replaces it before it can be
//!    written. Belt and braces — layer 1 is the real guarantee, layer 2
//!    catches the mistake layer 1 cannot (a `&str` logged under `draft = …`).

use std::fmt;

use tracing_subscriber::field::{RecordFields, Visit};
use tracing_subscriber::fmt::FormatFields;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Field names whose values are never written to logs. Grow this list
/// whenever a new P0-carrying field name appears in the codebase.
pub const P0_FIELD_DENYLIST: &[&str] = &[
    "draft",
    "text",
    "content",
    "payload",
    "suggestion",
    "transcript",
    "expressions",
    "humor",
    "yes_no_style",
];

/// Marker written instead of a denied value.
pub const REDACTED: &str = "[P0-REDACTED]";

/// Initializes process-wide tracing: env-filtered (`RUST_LOG`, default
/// `info`), compact format, **redacting field formatter**.
pub fn init() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().fmt_fields(RedactingFields))
        .init();
}

/// The redacting field formatter (public for tests: any subscriber built
/// with it inherits the guarantee).
#[derive(Debug, Clone, Copy)]
pub struct RedactingFields;

impl<'writer> FormatFields<'writer> for RedactingFields {
    fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> fmt::Result {
        let mut visitor = RedactingVisitor {
            writer,
            first: true,
            result: Ok(()),
        };
        fields.record(&mut visitor);
        visitor.result
    }
}

struct RedactingVisitor<'writer> {
    writer: Writer<'writer>,
    first: bool,
    result: fmt::Result,
}

impl RedactingVisitor<'_> {
    fn write_field(&mut self, name: &str, value: &dyn fmt::Debug) {
        if self.result.is_err() {
            return;
        }
        let separator = if self.first { "" } else { " " };
        self.first = false;
        self.result = if P0_FIELD_DENYLIST.contains(&name) {
            write!(self.writer, "{separator}{name}={REDACTED}")
        } else if name == "message" {
            write!(self.writer, "{separator}{value:?}")
        } else {
            write!(self.writer, "{separator}{name}={value:?}")
        };
    }
}

impl Visit for RedactingVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.write_field(field.name(), value);
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if P0_FIELD_DENYLIST.contains(&field.name()) {
            // Never let the raw string reach the formatting path at all.
            self.write_field(field.name(), &REDACTED);
        } else {
            self.write_field(field.name(), &value);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    /// A `MakeWriter` capturing everything into a shared buffer.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Capture {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().expect("lock").clone()).expect("utf8")
        }
    }

    impl std::io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn with_redacting_subscriber(f: impl FnOnce()) -> String {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .fmt_fields(RedactingFields)
                .with_writer(capture.clone())
                .with_ansi(false),
        );
        tracing::subscriber::with_default(subscriber, f);
        capture.contents()
    }

    /// PLAN 2.1: « un log contenant un champ P0 marqué = test rouge ».
    #[test]
    fn p0_named_fields_never_reach_the_output() {
        let output = with_redacting_subscriber(|| {
            tracing::info!(draft = "je voudrais de l'eau", "draft autosaved");
            tracing::info!(text = "contenu intime", caret = 12, "turn ingested");
        });
        assert!(!output.contains("je voudrais"), "P0 leaked: {output}");
        assert!(!output.contains("contenu intime"), "P0 leaked: {output}");
        assert!(output.contains(REDACTED));
        // Non-P0 fields keep flowing normally.
        assert!(output.contains("caret=12"));
    }

    #[test]
    fn secret_string_debug_never_prints_content() {
        use secrecy::SecretString;
        let secret = SecretString::from("phrase intime");
        let output = with_redacting_subscriber(|| {
            // Even logged under an innocent field name, the type protects.
            tracing::info!(value = ?secret, "wrapped secret");
        });
        assert!(!output.contains("phrase intime"), "P0 leaked: {output}");
    }

    #[test]
    fn ordinary_logging_is_untouched() {
        let output = with_redacting_subscriber(|| {
            tracing::info!(port = 7411, "hub listening");
        });
        assert!(output.contains("hub listening"));
        assert!(output.contains("port=7411"));
    }
}
