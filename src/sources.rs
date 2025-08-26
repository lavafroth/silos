use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

pub fn rule_files<P: AsRef<Path>>(path: P) -> io::Result<HashMap<String, Vec<PathBuf>>> {
    let per_language_dirs: Vec<_> = fs::read_dir(path)?
        .filter_map(|res| res.ok())
        .map(|direntry| direntry.path())
        .filter(|dir| dir.is_dir())
        .collect();

    let mut basename_to_paths = HashMap::new();

    for language_dir in per_language_dirs {
        let Some(dirname) = language_dir
            .file_stem()
            .and_then(|v| v.to_str())
            .map(|v| v.to_string())
        else {
            continue;
        };
        let rule_file_paths: Vec<_> = fs::read_dir(&language_dir)?
            .filter_map(|res| res.ok())
            .map(|entry| entry.path())
            .filter(|file| file.is_file() && file.extension().is_some_and(|ext| ext == "kdl"))
            .map(|path| path.to_path_buf())
            .collect();
        basename_to_paths.insert(dirname, rule_file_paths);
    }
    Ok(basename_to_paths)
}
// fn prebuilt_index();
