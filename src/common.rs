use std::{io::IsTerminal, sync::Mutex};

use slog::{o, Drain, Logger};

pub fn make_log(name: &'static str, debug_var: &'static str) -> Logger {
    let filter_level = match std::env::var(debug_var)
        .map(|v| v.to_ascii_lowercase())
        .as_deref()
    {
        Ok("yes") | Ok("1") | Ok("true") => slog::Level::Debug,
        _ => slog::Level::Info,
    };

    if std::io::stdout().is_terminal() {
        /*
         * Use a terminal-formatted logger for interactive processes.
         */
        let dec = slog_term::TermDecorator::new().stdout().build();
        let dr = Mutex::new(
            slog_term::FullFormat::new(dec).use_original_order().build(),
        )
        .filter_level(filter_level)
        .fuse();
        Logger::root(dr, o!("name" => name))
    } else {
        /*
         * Otherwise, emit bunyan-formatted records:
         */
        let dr = Mutex::new(
            slog_bunyan::with_name(name, std::io::stdout())
                .set_flush(true)
                .build(),
        )
        .filter_level(filter_level)
        .fuse();
        Logger::root(dr, o!())
    }
}

pub trait OutputExt {
    fn info(&self) -> String;
}

impl OutputExt for std::process::Output {
    fn info(&self) -> String {
        let mut out = String::new();

        if let Some(code) = self.status.code() {
            out.push_str(&format!("exit code {}", code));
        }

        /*
         * Attempt to render stderr from the command:
         */
        let stderr = String::from_utf8_lossy(&self.stderr).trim().to_string();
        let extra = if stderr.is_empty() {
            /*
             * If there is no stderr output, this command might emit its
             * failure message on stdout:
             */
            String::from_utf8_lossy(&self.stdout).trim().to_string()
        } else {
            stderr
        };

        if !extra.is_empty() {
            if !out.is_empty() {
                out.push_str(": ");
            }
            out.push_str(&extra);
        }

        out
    }
}
