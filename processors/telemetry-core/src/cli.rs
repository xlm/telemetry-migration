use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
pub struct FileArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long)]
    pub dlq: PathBuf,
    #[arg(long)]
    pub log: PathBuf,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::FileArgs;

    #[test]
    fn parses_required_file_arguments() {
        let args = FileArgs::try_parse_from([
            "processor",
            "--input",
            "in.jsonl",
            "--output",
            "out.jsonl",
            "--dlq",
            "dlq.jsonl",
            "--log",
            "run.log",
        ])
        .expect("valid file arguments");

        assert_eq!(args.input, std::path::PathBuf::from("in.jsonl"));
        assert_eq!(args.output, std::path::PathBuf::from("out.jsonl"));
        assert_eq!(args.dlq, std::path::PathBuf::from("dlq.jsonl"));
        assert_eq!(args.log, std::path::PathBuf::from("run.log"));
    }

    #[test]
    fn rejects_missing_file_arguments() {
        assert!(FileArgs::try_parse_from(["processor"]).is_err());
    }
}
