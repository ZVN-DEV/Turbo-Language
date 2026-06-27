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
        functions: &[
            "print",
            "read_line",
            "read_file",
            "write_file",
            "try_read_file",
            "try_write_file",
        ],
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
            "substring",
            "pad_left",
            "pad_right",
            "str_to_int",
            "str_to_float",
            "str_from_char",
        ],
    },
    StdlibModule {
        path: "std/array",
        functions: &["len", "push", "sort", "reverse", "array_contains", "slice"],
    },
    StdlibModule {
        path: "std/functional",
        functions: &["map", "filter", "reduce", "any", "all"],
    },
    StdlibModule {
        path: "std/math",
        functions: &[
            "abs",
            "min",
            "max",
            "pow",
            "sqrt",
            "float_to_int",
            "int_to_float",
            "random",
            "random_range",
        ],
    },
    StdlibModule {
        path: "std/fs",
        functions: &[
            "file_exists",
            "delete_file",
            "list_dir",
            "mkdir",
            "path_join",
            "path_dir",
            "path_base",
            "path_ext",
        ],
    },
    StdlibModule {
        path: "std/system",
        functions: &["shell_exec", "exec", "env_get", "args", "type_of"],
    },
    StdlibModule {
        path: "std/collections",
        functions: &[
            "hashmap",
            "hashmap_set",
            "hashmap_get",
            "hashmap_set_int",
            "hashmap_get_int",
            "hashmap_has",
            "hashmap_len",
            "hashmap_size",
            "hashmap_keys",
            "hashmap_remove",
        ],
    },
    StdlibModule {
        path: "std/json",
        functions: &[
            "json_get",
            "json_stringify",
            "json_build",
            "to_json",
            "to_json_array",
        ],
    },
    StdlibModule {
        path: "std/http",
        functions: &["http_get", "http_post", "http_post_with_headers"],
    },
    StdlibModule {
        path: "std/http/server",
        functions: &[
            "http_server",
            "http_server_public",
            "route",
            "http_listen",
            "respond",
            "respond_text",
            "respond_html",
            "respond_json",
            "request_body",
            "request_method",
            "request_path",
            "request_query",
            "request_header",
        ],
    },
    StdlibModule {
        path: "std/time",
        functions: &["time_now", "time_ms", "format_time"],
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
        assert!(m.functions.contains(&"http_server_public"));
        assert!(m.functions.contains(&"route"));
        assert!(m.functions.contains(&"respond"));
        assert!(m.functions.contains(&"respond_json"));
    }

    #[test]
    fn test_find_stdlib_module_shipped_builtin_expansions() {
        assert!(find_stdlib_module("std/io")
            .unwrap()
            .functions
            .contains(&"try_read_file"));
        assert!(find_stdlib_module("std/json")
            .unwrap()
            .functions
            .contains(&"json_build"));
        assert!(find_stdlib_module("std/collections")
            .unwrap()
            .functions
            .contains(&"hashmap_get_int"));
        assert!(find_stdlib_module("std/fs")
            .unwrap()
            .functions
            .contains(&"path_join"));
        assert!(find_stdlib_module("std/system")
            .unwrap()
            .functions
            .contains(&"shell_exec"));
        assert!(find_stdlib_module("std/time")
            .unwrap()
            .functions
            .contains(&"format_time"));
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
        assert_eq!(STDLIB_MODULES.len(), 15);
        let paths: Vec<&str> = STDLIB_MODULES.iter().map(|m| m.path).collect();
        assert!(paths.contains(&"std/io"));
        assert!(paths.contains(&"std/string"));
        assert!(paths.contains(&"std/array"));
        assert!(paths.contains(&"std/functional"));
        assert!(paths.contains(&"std/math"));
        assert!(paths.contains(&"std/fs"));
        assert!(paths.contains(&"std/system"));
        assert!(paths.contains(&"std/collections"));
        assert!(paths.contains(&"std/json"));
        assert!(paths.contains(&"std/http"));
        assert!(paths.contains(&"std/http/server"));
        assert!(paths.contains(&"std/time"));
        assert!(paths.contains(&"std/concurrency"));
        assert!(paths.contains(&"std/test"));
        assert!(paths.contains(&"std/unsafe"));
    }
}
