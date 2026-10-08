//! A time-limited local experiment, not an account API or background service.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tgsum_core::telegram_debug::capture::{CaptureConfig, CaptureLimits, CaptureSession};
use tgsum_core::telegram_debug::parser::{PeerKind, TypedPeer};

const HELP: &str = "Пробный сбор сообщений из диагностических логов Telegram Desktop 7.2.5.

telegram-debug-capture \\
  --logs /путь/к/DebugLogs --output /новая/приватная/папка \\
  --account primary --peer channel:123 --client-version 7.2.5 \\
  --single-account-confirmed [--seconds 60] [--self-user-id 123]

Программа читает только указанные локальные файлы. Telegram не запускает,
в аккаунт не входит, API не вызывает, сообщения не отправляет.
Поддерживается один подтверждённый аккаунт и один чат: user:ID, chat:ID,
channel:ID. Имена чатов для сопоставления не используются.
Срок эксперимента: 1–300 секунд. История и отсутствие пропусков не гарантируются.
Журнал содержит исходный текст выбранного чата; это приватные данные.
Полные диагностические логи Telegram могут содержать остальные чаты и поля входа.
По завершении отдельно выключите режим отладки в Telegram.
";

struct Arguments {
    logs: PathBuf,
    output: PathBuf,
    account: String,
    peer: TypedPeer,
    self_user_id: Option<String>,
    seconds: u64,
}

fn arguments(input: impl Iterator<Item = String>) -> Result<Option<Arguments>, &'static str> {
    let mut input = input.peekable();
    if input.peek().is_some_and(|v| v == "--help" || v == "-h") {
        input.next();
        if input.next().is_some() {
            return Err("--help должен быть указан отдельно");
        }
        return Ok(None);
    }
    let mut values = BTreeMap::new();
    let mut single_account = false;
    while let Some(flag) = input.next() {
        if flag == "--single-account-confirmed" {
            if single_account {
                return Err("Параметр подтверждения аккаунта повторяется");
            }
            single_account = true;
            continue;
        }
        if !matches!(
            flag.as_str(),
            "--logs"
                | "--output"
                | "--account"
                | "--peer"
                | "--client-version"
                | "--seconds"
                | "--self-user-id"
        ) {
            return Err("Неизвестный параметр; используйте --help");
        }
        let value = input.next().ok_or("Не задано значение параметра")?;
        if value.starts_with("--") || value.is_empty() {
            return Err("Не задано значение параметра");
        }
        if values.insert(flag, value).is_some() {
            return Err("Параметр повторяется");
        }
    }
    if !single_account {
        return Err("Сначала подтвердите, что в источнике ровно один аккаунт");
    }
    if values.remove("--client-version").as_deref() != Some("7.2.5") {
        return Err("Этот эксперимент рассчитан только на Telegram Desktop 7.2.5");
    }
    let logs = values.remove("--logs").ok_or("Не задана папка DebugLogs")?;
    let output = values
        .remove("--output")
        .ok_or("Не задана новая папка результата")?;
    let account = values
        .remove("--account")
        .ok_or("Не задано локальное имя аккаунта")?;
    if account.is_empty()
        || account.len() > 80
        || !account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("Имя аккаунта: до 80 латинских букв, цифр, знаков _ или -");
    }
    let peer = values
        .remove("--peer")
        .ok_or("Не задан ID выбранного чата")?;
    let (kind, id) = peer.split_once(':').ok_or("Формат чата: channel:ID")?;
    let kind = match kind {
        "channel" => PeerKind::Channel,
        "chat" => PeerKind::Chat,
        "user" => PeerKind::User,
        _ => return Err("Неизвестный тип чата"),
    };
    if !valid_id(id) {
        return Err("ID чата должен быть положительным десятичным числом");
    }
    let self_user_id = values.remove("--self-user-id");
    if self_user_id.as_deref().is_some_and(|id| !valid_id(id)) {
        return Err("Некорректный ID своего пользователя");
    }
    let seconds = values
        .remove("--seconds")
        .map(|s| s.parse::<u64>().map_err(|_| "Некорректная длительность"))
        .transpose()?
        .unwrap_or(60);
    if !(1..=300).contains(&seconds) {
        return Err("Длительность должна быть от 1 до 300 секунд");
    }
    Ok(Some(Arguments {
        logs: logs.into(),
        output: output.into(),
        account,
        peer: TypedPeer {
            kind,
            id: id.into(),
        },
        self_user_id,
        seconds,
    }))
}

fn valid_id(value: &str) -> bool {
    !value.starts_with('0')
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<i64>().is_ok_and(|v| v > 0)
}

fn run(args: Arguments) -> Result<(), Box<dyn std::error::Error>> {
    let config = CaptureConfig {
        input_directory: args.logs,
        output_directory: args.output,
        account_namespace: args.account,
        peer: args.peer,
        self_user_id: args.self_user_id,
        confirmed_single_account: true,
        limits: CaptureLimits::default(),
    };
    let mut capture = CaptureSession::open(config)?;
    let started = Instant::now();
    let duration = Duration::from_secs(args.seconds);
    let mut next_report = Duration::ZERO;
    loop {
        let report = capture.poll()?;
        let elapsed = started.elapsed();
        if elapsed >= next_report || elapsed >= duration {
            println!("{}", serde_json::to_string(&report)?);
            next_report = elapsed + Duration::from_secs(5);
        }
        if elapsed >= duration {
            break;
        }
        std::thread::sleep(Duration::from_millis(250).min(duration.saturating_sub(elapsed)));
    }
    println!("Эксперимент остановлен. Полнота переписки не подтверждена.");
    Ok(())
}

fn main() -> std::process::ExitCode {
    match arguments(std::env::args().skip(1)) {
        Ok(None) => {
            println!("{HELP}");
            std::process::ExitCode::SUCCESS
        }
        Ok(Some(args)) => match run(args) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Сбор остановлен: {error}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Arguments>, &'static str> {
        arguments(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn unsupported_version_and_unbound_account_stop_before_io() {
        assert!(parse(&["--client-version", "7.2.5"]).is_err());
        assert!(parse(&["--single-account-confirmed", "--client-version", "7.2.6"]).is_err());
    }

    #[test]
    fn duration_is_bounded_and_identity_is_exact() {
        let args = [
            "--logs",
            "/logs",
            "--output",
            "/new",
            "--account",
            "test",
            "--peer",
            "channel:9007199254740993",
            "--client-version",
            "7.2.5",
            "--single-account-confirmed",
            "--seconds",
            "1",
        ];
        let parsed = parse(&args).unwrap().unwrap();
        assert_eq!(parsed.peer.id, "9007199254740993");
        let mut too_long = args;
        too_long[12] = "301";
        assert!(parse(&too_long).is_err());
        let mut invalid = args;
        invalid[7] = "channel:1.2";
        assert!(parse(&invalid).is_err());
    }
}
