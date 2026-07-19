use std::path::PathBuf;
use std::process::ExitCode;

use handball_toolkit_cli::corpus::validate_corpus;
use handball_toolkit_cli::report::RunReport;
use handball_toolkit_cli::validate::validate_file;

const USAGE: &str = "\
usage: handball-toolkit-cli validate [--json] <path>...

  <path> がディレクトリなら v2 ルートとして一括検証する
  （index.json / matches/*.json / highlights/ を index と突合）。
  ファイルならトップレベルキーで形状を自動判別して単体検証する
  （facts = 試合本体 / matches = 試合 index / highlights = ハイライト index）。

  --json  結果を JSON（{checkedFiles, findings[]}）で stdout に出力する

exit code: 0 = 指摘なし / 1 = 指摘あり / 2 = 使い方・パス誤り";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    run(&args)
}

fn run(args: &[String]) -> ExitCode {
    let Some((command, rest)) = args.split_first() else {
        return usage_error("subcommand がありません");
    };
    if command != "validate" {
        return usage_error(&format!("未知の subcommand: {command}"));
    }

    let mut json_output = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    for arg in rest {
        match arg.as_str() {
            "--json" => json_output = true,
            flag if flag.starts_with('-') => {
                return usage_error(&format!("未知のフラグ: {flag}"));
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    if paths.is_empty() {
        return usage_error("検証対象のパスがありません");
    }

    let mut report = RunReport::default();
    for path in &paths {
        if path.is_dir() {
            validate_corpus(path, &mut report);
        } else if path.is_file() {
            validate_file(path, &mut report);
        } else {
            eprintln!("パスが存在しません: {}", path.display());
            return ExitCode::from(2);
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("RunReport は常に serialize 可能")
        );
    } else {
        for finding in &report.findings {
            println!("{}", finding.human_line());
        }
        println!(
            "検証 {} ファイル / 指摘 {} 件",
            report.checked_files,
            report.finding_count()
        );
    }

    if report.findings.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("{message}\n\n{USAGE}");
    ExitCode::from(2)
}
