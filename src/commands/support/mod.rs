//! Portable member paths for command-line examples.
use std::{io, path::PathBuf};

pub fn member_path(name: &str) -> io::Result<PathBuf> {
    let mut path = PathBuf::new();
    for part in name.split(['/', '\\']) {
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'));
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
            || reserved
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsafe or non-portable CAB member name: {name:?}"),
            ));
        }
        path.push(part);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn member_paths_reject_traversal_absolute_and_windows_aliases() {
        for name in [
            "",
            "../escape",
            "/absolute",
            "\\absolute",
            "a//b",
            "a/./b",
            "C:\\file",
            "file:stream",
            "NUL",
            "dir/COM1.txt",
            "trailing.",
            "trailing ",
        ] {
            assert!(member_path(name).is_err(), "{name:?}");
        }
        assert_eq!(
            member_path("dir\\é.txt").unwrap(),
            PathBuf::from("dir").join("é.txt")
        );
    }
}
