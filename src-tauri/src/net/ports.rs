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
    let mut last: Option<std::io::Error> = None;
    for attempt in 0..3 {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                drop(listener);
                return Ok(());
            }
            Err(error) => {
                last = Some(error);
                if attempt < 2 {
                    // A port the last kernel used stays unusable for the length
                    // of its TIME_WAIT (measured on the test box: 10013 for
                    // about a minute). Three tries over 600 ms covers the short
                    // tail of that and costs nothing when the port is busy.
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            }
        }
    }
    let error = last.expect("at least one bind attempt");
    // Binding alone cannot tell "somebody is listening" from "the system still
    // holds a socket in TIME_WAIT". The user needs the difference: one is
    // another client to close, the other is time.
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let occupied = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).is_ok();
    eprintln!("x-matrix: {what} {port} 绑定失败：{error}");
    if occupied {
        Err(AppError::Engine(format!(
            "{what} {port} 已被占用：可能是另一个代理客户端还在跑。先关掉它，或在设置里换一个端口"
        )))
    } else {
        Err(AppError::Engine(format!(
            "{what} {port} 刚被别的程序用过，系统还没放（通常几十秒）：等一会儿再试，或在设置里换一个端口"
        )))
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
