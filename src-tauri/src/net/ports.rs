//! Port preflight.
//!
//! A kernel that cannot bind its inbound does not say so on our side: it dies
//! with a line inside its own log file, in wording the user has to go hunting
//! for. On a desktop the usual cause is another proxy client still holding
//! 7890 — which is exactly the situation this project keeps running into — so
//! we look before spawning and say it in one sentence.

use std::net::TcpListener;

use crate::error::{AppError, AppResult};

/// Can we bind `127.0.0.1:port` right now?
///
/// Binding is the only honest test: asking the OS "is this port free" and then
/// binding later leaves a window where the answer changed. The listener is
/// dropped immediately, so the port is free again for the kernel to take.
pub fn ensure_free(port: u16, what: &str) -> AppResult<()> {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => {
            drop(listener);
            Ok(())
        }
        Err(error) => Err(AppError::Engine(format!(
            "{what} {port} 已被占用（{error}）：可能是另一个代理客户端还在跑，先关掉它，或在设置里换一个端口"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grab a port the OS just handed us, so the test never fights a fixed one.
    fn occupy() -> (TcpListener, u16) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
        let port = listener.local_addr().expect("addr").port();
        (listener, port)
    }

    #[test]
    fn a_port_somebody_else_holds_is_reported_with_its_number() {
        let (_holder, port) = occupy();
        let error = ensure_free(port, "混合端口").expect_err("port is taken");
        let message = error.to_string();
        assert!(message.contains(&port.to_string()), "message names the port: {message}");
        assert!(message.contains("混合端口"), "message names the field: {message}");
    }

    #[test]
    fn a_port_we_just_released_is_free() {
        let (holder, port) = occupy();
        drop(holder);
        // Nothing promises the kernel hands the port back instantly, but a
        // listener we closed ourselves is not held by anyone, so the bind must
        // succeed — if this ever flakes, the preflight is too strict, not the
        // test.
        ensure_free(port, "控制端口").expect("released port is free");
    }
}
