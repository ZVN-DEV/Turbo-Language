/// Virtual standard library module definitions.
/// Maps module path (e.g. "std/string") to the builtin function names it exports.
///
/// These are purely organizational -- all builtins remain globally available
/// without imports. The import syntax `import { trim } from "std/string"`
/// is validated at compile time and then discarded.
pub struct StdlibModule {
    pub path: &'static str,
    pub functions: &'static [&'static str],
}

pub const STDLIB_MODULES: &[StdlibModule] = &[
    StdlibModule {
        path: "std/io",
        functions: &["print", "read_line", "read_file", "write_file"],
    },
    StdlibModule {
        path: "std/string",
        functions: &[
            "len",
            "trim",
            "upper",
            "lower",
            "split",
            "contains",
            "starts_with",
            "ends_with",
            "replace",
            "index_of",
            "char_at",
            "repeat",
            "join",
            "to_str",
        ],
    },
    StdlibModule {
        path: "std/array",
        functions: &["len", "push"],
    },
    StdlibModule {
        path: "std/functional",
        functions: &["map", "filter", "reduce"],
    },
    StdlibModule {
        path: "std/math",
        functions: &["abs", "min", "max", "pow", "sqrt"],
    },
    StdlibModule {
        path: "std/collections",
        functions: &[
            "hashmap",
            "hashmap_set",
            "hashmap_get",
            "hashmap_has",
            "hashmap_len",
            "hashmap_size",
            "hashmap_keys",
            "hashmap_remove",
        ],
    },
    StdlibModule {
        path: "std/json",
        functions: &["json_get", "json_stringify", "to_json", "to_json_array"],
    },
    StdlibModule {
        path: "std/http",
        functions: &["http_get", "http_post"],
    },
    StdlibModule {
        path: "std/http/server",
        functions: &[
            "http_server",
            "route",
            "http_listen",
            "respond",
            "request_body",
            "request_method",
            "request_path",
            "request_query",
            "request_header",
        ],
    },
    StdlibModule {
        path: "std/concurrency",
        functions: &[
            "channel",
            "send",
            "recv",
            "mutex",
            "mutex_get",
            "mutex_set",
            "sleep",
            "clone",
        ],
    },
    StdlibModule {
        path: "std/test",
        functions: &["assert", "assert_eq", "assert_ne", "panic"],
    },
    StdlibModule {
        path: "std/unsafe",
        functions: &["deref", "store"],
    },
];

/// Look up a stdlib module by path. Returns None for non-std paths.
pub fn find_stdlib_module(path: &str) -> Option<&'static StdlibModule> {
    STDLIB_MODULES.iter().find(|m| m.path == path)
}

/// Check if a path is a stdlib module path (starts with "std/").
pub fn is_stdlib_path(path: &str) -> bool {
    path.starts_with("std/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_stdlib_module() {
        let m = find_stdlib_module("std/string").unwrap();
        assert!(m.functions.contains(&"trim"));
        assert!(m.functions.contains(&"len"));
        assert!(!m.functions.contains(&"hashmap"));
    }

    #[test]
    fn test_find_stdlib_module_http_server() {
        let m = find_stdlib_module("std/http/server").unwrap();
        assert!(m.functions.contains(&"http_server"));
        assert!(m.functions.contains(&"route"));
        assert!(m.functions.contains(&"respond"));
    }

    #[test]
    fn test_unknown_module() {
        assert!(find_stdlib_module("std/foo").is_none());
        assert!(find_stdlib_module("string").is_none());
        assert!(find_stdlib_module("").is_none());
    }

    #[test]
    fn test_is_stdlib_path() {
        assert!(is_stdlib_path("std/string"));
        assert!(is_stdlib_path("std/http/server"));
        assert!(!is_stdlib_path("./utils"));
        assert!(!is_stdlib_path("mathlib"));
        assert!(!is_stdlib_path(""));
    }

    #[test]
    fn test_len_in_both_modules() {
        let string_mod = find_stdlib_module("std/string").unwrap();
        let array_mod = find_stdlib_module("std/array").unwrap();
        assert!(string_mod.functions.contains(&"len"));
        assert!(array_mod.functions.contains(&"len"));
    }

    #[test]
    fn test_all_modules_present() {
        assert_eq!(STDLIB_MODULES.len(), 12);
        let paths: Vec<&str> = STDLIB_MODULES.iter().map(|m| m.path).collect();
        assert!(paths.contains(&"std/io"));
        assert!(paths.contains(&"std/string"));
        assert!(paths.contains(&"std/array"));
        assert!(paths.contains(&"std/functional"));
        assert!(paths.contains(&"std/math"));
        assert!(paths.contains(&"std/collections"));
        assert!(paths.contains(&"std/json"));
        assert!(paths.contains(&"std/http"));
        assert!(paths.contains(&"std/http/server"));
        assert!(paths.contains(&"std/concurrency"));
        assert!(paths.contains(&"std/test"));
        assert!(paths.contains(&"std/unsafe"));
    }
}
