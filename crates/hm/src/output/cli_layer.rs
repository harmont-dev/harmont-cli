use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

#[derive(Debug, Clone)]
pub struct CliLayer<O, E> {
    stdout_sink: Arc<Mutex<O>>,
    stderr_sink: Arc<Mutex<E>>,
}

impl CliLayer<std::io::Stdout, std::io::Stderr> {
    pub fn real() -> Self {
        Self {
            stdout_sink: Arc::new(Mutex::new(std::io::stdout())),
            stderr_sink: Arc::new(Mutex::new(std::io::stderr())),
        }
    }
}

impl<O, E> CliLayer<O, E> {
    pub fn with_sinks(stdout: O, stderr: E) -> Self {
        Self {
            stdout_sink: Arc::new(Mutex::new(stdout)),
            stderr_sink: Arc::new(Mutex::new(stderr)),
        }
    }
}

#[derive(Default)]
struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            write!(self.0, "{value:?}").ok();
        }
    }
}

impl<S, O, E> Layer<S> for CliLayer<O, E>
where
    S: Subscriber,
    O: Write + Send + 'static,
    E: Write + Send + 'static,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let target = event.metadata().target();

        let is_stdout = target == "user::stdout";
        let is_stderr = target == "user::stderr";
        if !is_stdout && !is_stderr {
            return;
        }

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let msg = visitor.0;

        if is_stdout {
            if let Ok(mut w) = self.stdout_sink.lock() {
                writeln!(w, "{msg}").ok();
            }
        } else if let Ok(mut w) = self.stderr_sink.lock() {
            writeln!(w, "{msg}").ok();
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    fn capture_layer() -> (
        CliLayer<Vec<u8>, Vec<u8>>,
        Arc<Mutex<Vec<u8>>>,
        Arc<Mutex<Vec<u8>>>,
    ) {
        let stdout_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let layer = CliLayer {
            stdout_sink: Arc::clone(&stdout_buf),
            stderr_sink: Arc::clone(&stderr_buf),
        };
        (layer, stdout_buf, stderr_buf)
    }

    fn buf_str(buf: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buf.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn routes_stdout_target_to_stdout_sink() {
        let (layer, stdout_buf, stderr_buf) = capture_layer();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);
        tracing::info!(target: "user::stdout", "hello world");
        assert_eq!(buf_str(&stdout_buf), "hello world\n");
        assert!(buf_str(&stderr_buf).is_empty());
    }

    #[test]
    fn routes_stderr_target_to_stderr_sink() {
        let (layer, stdout_buf, stderr_buf) = capture_layer();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);
        tracing::info!(target: "user::stderr", "warning msg");
        assert!(buf_str(&stdout_buf).is_empty());
        assert_eq!(buf_str(&stderr_buf), "warning msg\n");
    }

    #[test]
    fn ignores_other_targets() {
        let (layer, stdout_buf, stderr_buf) = capture_layer();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);
        tracing::info!("diagnostic message");
        assert!(buf_str(&stdout_buf).is_empty());
        assert!(buf_str(&stderr_buf).is_empty());
    }

    #[test]
    fn handles_format_args() {
        let (layer, stdout_buf, _) = capture_layer();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);
        let name = "alice";
        let count = 42;
        tracing::info!(target: "user::stdout", "{name} has {count} items");
        assert_eq!(buf_str(&stdout_buf), "alice has 42 items\n");
    }

    #[test]
    fn handles_empty_message() {
        let (layer, stdout_buf, _) = capture_layer();
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);
        tracing::info!(target: "user::stdout", "");
        assert_eq!(buf_str(&stdout_buf), "\n");
    }
}
