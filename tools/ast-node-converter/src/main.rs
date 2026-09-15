use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

fn main() {
    let mut args = env::args_os().skip(1).collect::<Vec<_>>();
    let stdin = take_flag(&mut args, "--stdin");
    let embedded = take_flag(&mut args, "--rust-strings");
    let format = take_flag(&mut args, "--format");
    let markdown = take_flag(&mut args, "--markdown");
    if stdin {
        let mut source = String::new();
        io::stdin().read_to_string(&mut source).unwrap();
        let (mut output, report) = recursa_ast_node_converter::convert_source(&source).unwrap();
        if format {
            output = recursa_ast_node_converter::format_source(&output).unwrap();
        }
        print!("{output}");
        eprintln!(
            "converted {} declarations, {} containers, {} lifetime occurrences, {} omissions; {} unsupported",
            report.declarations,
            report.containers,
            report.lifetimes,
            report.omissions,
            report.unsupported.len()
        );
        for site in &report.unsupported {
            eprintln!("unsupported: {site}");
        }
        if !report.unsupported.is_empty() {
            std::process::exit(1);
        }
        return;
    }
    if args.is_empty() {
        eprintln!(
            "usage: ast-node-converter [--stdin] [--rust-strings] [--markdown] [--format] PATH..."
        );
        std::process::exit(2);
    }
    let mut files = Vec::new();
    for arg in args {
        if markdown {
            files.push(PathBuf::from(arg));
        } else {
            collect(Path::new(&arg), &mut files);
        }
    }
    files.sort();
    files.dedup();
    let mut total = recursa_ast_node_converter::Report::default();
    for path in files {
        let source = fs::read_to_string(&path).unwrap();
        let (mut output, report) = if markdown {
            Ok::<_, syn::Error>((
                recursa_ast_node_converter::format_markdown(&source).unwrap(),
                Default::default(),
            ))
        } else if embedded {
            recursa_ast_node_converter::convert_embedded_sources(&source)
        } else {
            recursa_ast_node_converter::convert_source(&source)
        }
        .unwrap();
        if format {
            output = recursa_ast_node_converter::format_source(&output).unwrap();
        }
        if output != source {
            fs::write(&path, output).unwrap();
        }
        total.declarations += report.declarations;
        total.containers += report.containers;
        total.lifetimes += report.lifetimes;
        total.omissions += report.omissions;
        total.unsupported.extend(
            report
                .unsupported
                .into_iter()
                .map(|site| format!("{}: {site}", path.display())),
        );
    }
    eprintln!(
        "converted {} declarations in one invocation, {} containers, {} lifetime occurrences, {} omissions; {} unsupported",
        total.declarations,
        total.containers,
        total.lifetimes,
        total.omissions,
        total.unsupported.len()
    );
    for site in &total.unsupported {
        eprintln!("unsupported: {site}");
    }
    if !total.unsupported.is_empty() {
        std::process::exit(1);
    }
}

fn take_flag(args: &mut Vec<std::ffi::OsString>, flag: &str) -> bool {
    if let Some(index) = args.iter().position(|arg| arg == flag) {
        args.remove(index);
        true
    } else {
        false
    }
}

fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        if path
            .file_name()
            .is_some_and(|name| name == "target" || name == ".git")
        {
            return;
        }
        for entry in fs::read_dir(path).unwrap() {
            collect(&entry.unwrap().path(), files);
        }
    } else if path.extension().is_some_and(|ext| ext == "rs")
        || path.to_string_lossy().ends_with(".rs.in")
    {
        files.push(path.to_owned());
    }
}
