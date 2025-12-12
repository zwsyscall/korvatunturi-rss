mod commands;
mod server;

pub use commands::ServerCommand;
pub use server::start;

#[macro_export]
macro_rules! reply_err {
    ($tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        log::error!("{}", &msg);
        if $tx.send(msg).is_err() {
            error!("Error sending reply, channel closed");
        }
    }};
}

#[macro_export]
macro_rules! reply_ok {
    ($tx:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        log::debug!("{}", &msg);
        if $tx.send(msg).is_err() {
            error!("Error sending reply, channel closed");
        }
    }};
}
