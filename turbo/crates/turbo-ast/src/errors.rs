/// Production error codes for the Turbo compiler.
///
/// Ranges:
/// - E0001-E0099: Parse errors
/// - E0100-E0199: Type errors (mismatches, incompatible operations)
/// - E0200-E0299: Pattern/match errors (exhaustiveness, unreachable, guards)
/// - E0300-E0399: Name resolution (undefined var/fn/type, duplicates, scope)
/// - E0400-E0499: Codegen errors
/// - E0500-E0599: Misc (immutability, unused, constraints, etc.)
///
/// # Examples
///
/// ```
/// use turbo_ast::ErrorCode;
///
/// let code = ErrorCode::E0001;
/// assert_eq!(code.as_str(), "E0001");
/// assert!(!code.description().is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    // ── Parse errors (E0001-E0099) ──────────────────────────────────
    /// Unexpected token
    E0001,
    /// Expected identifier
    E0002,
    /// Maximum nesting depth exceeded
    E0003,
    /// Invalid attribute
    E0007,

    // ── Type errors (E0100-E0199) ───────────────────────────────────
    /// Type mismatch (general)
    E0100,
    /// Cannot perform arithmetic on non-numeric type
    E0101,
    /// Mismatched types in arithmetic
    E0102,
    /// Cannot compare incompatible types
    E0103,
    /// Expected bool in logical operation
    E0104,
    /// Cannot negate type
    E0105,
    /// Cannot apply logical not to non-bool
    E0106,
    /// If/else branches have different types
    E0107,
    /// Match arms have different types
    E0108,
    /// Return type mismatch
    E0109,
    /// Type annotation doesn't match value
    E0110,
    /// Cannot assign incompatible type to variable
    E0111,
    /// Cannot assign incompatible type to field
    E0112,
    /// Cannot assign incompatible type to array element
    E0113,
    /// Array elements must have the same type
    E0114,
    /// Cannot infer type of empty array
    E0115,
    /// If condition must be bool
    E0116,
    /// While condition must be bool
    E0117,
    /// Null coalesce type mismatch
    E0118,
    /// Null coalesce requires optional type
    E0119,
    /// Try operator requires Result type
    E0120,
    /// Try operator in non-Result function
    E0121,
    /// Range bounds must be integer
    E0122,
    /// Array index must be integer
    E0123,
    /// Cannot index into non-array type
    E0124,
    /// Closure body type mismatch
    E0125,
    /// Cannot infer closure parameter type
    E0126,
    /// Unknown closure return type
    E0127,
    /// Struct equality requires derive(Eq)
    E0128,
    /// Cannot use ordering comparison on struct
    E0129,
    /// Incompatible compound assignment
    E0130,
    /// Type parameter inference conflict
    E0131,
    /// Pattern type incompatible with subject
    E0132,
    /// Builtin function argument type mismatch
    E0133,
    /// Cannot call method on type
    E0134,
    /// Cannot access field on type
    E0135,
    /// Expression nesting too deep
    E0136,
    /// Unsupported compound assignment operator
    E0137,

    // ── Pattern/match errors (E0200-E0299) ──────────────────────────
    /// Match is not exhaustive
    E0200,
    /// Match expression has no arms
    E0201,
    /// Match guard must be bool
    E0202,

    // ── Name resolution errors (E0300-E0399) ────────────────────────
    /// Undefined variable
    E0300,
    /// Undefined function
    E0301,
    /// Undefined struct
    E0302,
    /// Undefined enum
    E0303,
    /// Undefined trait
    E0304,
    /// Unknown type
    E0305,
    /// Struct already defined (duplicate)
    E0306,
    /// Enum already defined (duplicate)
    E0307,
    /// Function already defined (duplicate)
    E0308,
    /// Trait already defined (duplicate)
    E0309,
    /// Method already defined (duplicate)
    E0310,
    /// Constant already defined (duplicate)
    E0311,
    /// Cannot redefine builtin function
    E0313,
    /// No main function found
    E0314,
    /// Struct has no field
    E0315,
    /// Enum has no variant
    E0316,
    /// No method found on type
    E0317,
    /// Missing field in struct literal
    E0318,
    /// Unknown derive trait
    E0319,
    /// Unknown type in trait method
    E0323,
    /// Unknown return type
    E0324,

    // ── Codegen errors (E0400-E0499) ────────────────────────────────
    /// Internal codegen error
    E0400,
    /// Undefined variable in codegen
    E0401,
    /// Undefined function in codegen
    E0402,
    /// Unsupported expression in codegen
    E0403,
    /// Linker/build error
    E0404,
    /// Cranelift error
    E0405,
    /// Missing function definition
    E0406,

    // ── Misc errors (E0500-E0599) ───────────────────────────────────
    /// Cannot assign to immutable variable
    E0501,
    /// Cannot assign to immutable field
    E0502,
    /// Cannot assign to immutable index
    E0503,
    /// Test function has parameters
    E0504,
    /// Test function has return type
    E0505,
    /// Constant must be a literal value
    E0506,
    /// Break outside loop
    E0507,
    /// Continue outside loop
    E0508,
    /// Trait method not implemented
    E0509,
    /// Trait impl return type mismatch
    E0510,
    /// Only named function calls supported
    E0512,
    /// Builtin argument count error
    E0513,
    /// Unused return value of pure function
    E0514,
    /// Unused variable
    E0515,
    /// Compiler recursion limit exceeded (parser/codegen)
    E0516,
}

impl ErrorCode {
    /// Returns the code string, e.g. "E0001".
    pub fn as_str(&self) -> &'static str {
        match self {
            // Parse
            ErrorCode::E0001 => "E0001",
            ErrorCode::E0002 => "E0002",
            ErrorCode::E0003 => "E0003",
            ErrorCode::E0007 => "E0007",
            // Type
            ErrorCode::E0100 => "E0100",
            ErrorCode::E0101 => "E0101",
            ErrorCode::E0102 => "E0102",
            ErrorCode::E0103 => "E0103",
            ErrorCode::E0104 => "E0104",
            ErrorCode::E0105 => "E0105",
            ErrorCode::E0106 => "E0106",
            ErrorCode::E0107 => "E0107",
            ErrorCode::E0108 => "E0108",
            ErrorCode::E0109 => "E0109",
            ErrorCode::E0110 => "E0110",
            ErrorCode::E0111 => "E0111",
            ErrorCode::E0112 => "E0112",
            ErrorCode::E0113 => "E0113",
            ErrorCode::E0114 => "E0114",
            ErrorCode::E0115 => "E0115",
            ErrorCode::E0116 => "E0116",
            ErrorCode::E0117 => "E0117",
            ErrorCode::E0118 => "E0118",
            ErrorCode::E0119 => "E0119",
            ErrorCode::E0120 => "E0120",
            ErrorCode::E0121 => "E0121",
            ErrorCode::E0122 => "E0122",
            ErrorCode::E0123 => "E0123",
            ErrorCode::E0124 => "E0124",
            ErrorCode::E0125 => "E0125",
            ErrorCode::E0126 => "E0126",
            ErrorCode::E0127 => "E0127",
            ErrorCode::E0128 => "E0128",
            ErrorCode::E0129 => "E0129",
            ErrorCode::E0130 => "E0130",
            ErrorCode::E0131 => "E0131",
            ErrorCode::E0132 => "E0132",
            ErrorCode::E0133 => "E0133",
            ErrorCode::E0134 => "E0134",
            ErrorCode::E0135 => "E0135",
            ErrorCode::E0136 => "E0136",
            ErrorCode::E0137 => "E0137",
            // Pattern/match
            ErrorCode::E0200 => "E0200",
            ErrorCode::E0201 => "E0201",
            ErrorCode::E0202 => "E0202",
            // Name resolution
            ErrorCode::E0300 => "E0300",
            ErrorCode::E0301 => "E0301",
            ErrorCode::E0302 => "E0302",
            ErrorCode::E0303 => "E0303",
            ErrorCode::E0304 => "E0304",
            ErrorCode::E0305 => "E0305",
            ErrorCode::E0306 => "E0306",
            ErrorCode::E0307 => "E0307",
            ErrorCode::E0308 => "E0308",
            ErrorCode::E0309 => "E0309",
            ErrorCode::E0310 => "E0310",
            ErrorCode::E0311 => "E0311",
            ErrorCode::E0313 => "E0313",
            ErrorCode::E0314 => "E0314",
            ErrorCode::E0315 => "E0315",
            ErrorCode::E0316 => "E0316",
            ErrorCode::E0317 => "E0317",
            ErrorCode::E0318 => "E0318",
            ErrorCode::E0319 => "E0319",
            ErrorCode::E0323 => "E0323",
            ErrorCode::E0324 => "E0324",
            // Codegen
            ErrorCode::E0400 => "E0400",
            ErrorCode::E0401 => "E0401",
            ErrorCode::E0402 => "E0402",
            ErrorCode::E0403 => "E0403",
            ErrorCode::E0404 => "E0404",
            ErrorCode::E0405 => "E0405",
            ErrorCode::E0406 => "E0406",
            // Misc
            ErrorCode::E0501 => "E0501",
            ErrorCode::E0502 => "E0502",
            ErrorCode::E0503 => "E0503",
            ErrorCode::E0504 => "E0504",
            ErrorCode::E0505 => "E0505",
            ErrorCode::E0506 => "E0506",
            ErrorCode::E0507 => "E0507",
            ErrorCode::E0508 => "E0508",
            ErrorCode::E0509 => "E0509",
            ErrorCode::E0510 => "E0510",
            ErrorCode::E0512 => "E0512",
            ErrorCode::E0513 => "E0513",
            ErrorCode::E0514 => "E0514",
            ErrorCode::E0515 => "E0515",
            ErrorCode::E0516 => "E0516",
        }
    }

    /// Returns a short human-readable description of the error code.
    pub fn description(&self) -> &'static str {
        match self {
            // Parse
            ErrorCode::E0001 => "unexpected token during parsing",
            ErrorCode::E0002 => "expected identifier",
            ErrorCode::E0003 => "maximum nesting depth exceeded",
            ErrorCode::E0007 => "invalid attribute",
            // Type
            ErrorCode::E0100 => "type mismatch",
            ErrorCode::E0101 => "cannot perform arithmetic on non-numeric type",
            ErrorCode::E0102 => "mismatched types in arithmetic operation",
            ErrorCode::E0103 => "cannot compare incompatible types",
            ErrorCode::E0104 => "expected `bool` in logical operation",
            ErrorCode::E0105 => "cannot negate non-numeric type",
            ErrorCode::E0106 => "cannot apply logical not to non-bool type",
            ErrorCode::E0107 => "if/else branches have different types",
            ErrorCode::E0108 => "match arms have different types",
            ErrorCode::E0109 => "function return type does not match body",
            ErrorCode::E0110 => "type annotation does not match value type",
            ErrorCode::E0111 => "cannot assign incompatible type to variable",
            ErrorCode::E0112 => "cannot assign incompatible type to struct field",
            ErrorCode::E0113 => "cannot assign incompatible type to array element",
            ErrorCode::E0114 => "array elements must all have the same type",
            ErrorCode::E0115 => "cannot infer type of empty array literal",
            ErrorCode::E0116 => "if condition must be `bool`",
            ErrorCode::E0117 => "while condition must be `bool`",
            ErrorCode::E0118 => "`??` inner type does not match default type",
            ErrorCode::E0119 => "`??` requires an optional type on the left",
            ErrorCode::E0120 => "`?` operator requires a Result type",
            ErrorCode::E0121 => "`?` can only be used in a function returning Result",
            ErrorCode::E0122 => "range bounds must be integers",
            ErrorCode::E0123 => "array index must be an integer",
            ErrorCode::E0124 => "cannot index into non-array type",
            ErrorCode::E0125 => "closure body type does not match declared return type",
            ErrorCode::E0126 => "cannot infer type of closure parameter",
            ErrorCode::E0127 => "unknown closure return type",
            ErrorCode::E0128 => "struct equality requires `@derive(Eq)`",
            ErrorCode::E0129 => "cannot use ordering comparison on struct type",
            ErrorCode::E0130 => "incompatible type in compound assignment",
            ErrorCode::E0131 => "type parameter inference conflict",
            ErrorCode::E0132 => "pattern type incompatible with match subject",
            ErrorCode::E0133 => "builtin function argument type mismatch",
            ErrorCode::E0134 => "cannot call method on this type",
            ErrorCode::E0135 => "cannot access field on this type",
            ErrorCode::E0136 => "expression nesting too deep",
            ErrorCode::E0137 => "unsupported compound assignment operator",
            // Pattern/match
            ErrorCode::E0200 => "match expression is not exhaustive",
            ErrorCode::E0201 => "match expression has no arms",
            ErrorCode::E0202 => "match guard must be `bool`",
            // Name resolution
            ErrorCode::E0300 => "undefined variable",
            ErrorCode::E0301 => "undefined function",
            ErrorCode::E0302 => "undefined struct",
            ErrorCode::E0303 => "undefined enum",
            ErrorCode::E0304 => "undefined trait",
            ErrorCode::E0305 => "unknown type name",
            ErrorCode::E0306 => "struct is already defined",
            ErrorCode::E0307 => "enum is already defined",
            ErrorCode::E0308 => "function is already defined",
            ErrorCode::E0309 => "trait is already defined",
            ErrorCode::E0310 => "method is already defined for this type",
            ErrorCode::E0311 => "constant is already defined",
            ErrorCode::E0313 => "cannot redefine builtin function",
            ErrorCode::E0314 => "no `main` function found",
            ErrorCode::E0315 => "struct has no such field",
            ErrorCode::E0316 => "enum has no such variant",
            ErrorCode::E0317 => "no such method found on type",
            ErrorCode::E0318 => "missing field in struct literal",
            ErrorCode::E0319 => "unknown derive trait",
            ErrorCode::E0323 => "unknown type in trait method signature",
            ErrorCode::E0324 => "unknown return type",
            // Codegen
            ErrorCode::E0400 => "internal code generation error",
            ErrorCode::E0401 => "undefined variable during code generation",
            ErrorCode::E0402 => "undefined function during code generation",
            ErrorCode::E0403 => "unsupported expression during code generation",
            ErrorCode::E0404 => "linker or build tool error",
            ErrorCode::E0405 => "Cranelift backend error",
            ErrorCode::E0406 => "missing function definition",
            // Misc
            ErrorCode::E0501 => "cannot assign to immutable variable",
            ErrorCode::E0502 => "cannot assign to field of immutable variable",
            ErrorCode::E0503 => "cannot assign to index of immutable variable",
            ErrorCode::E0504 => "test function must have no parameters",
            ErrorCode::E0505 => "test function must have no return type",
            ErrorCode::E0506 => "constant must be a literal value",
            ErrorCode::E0507 => "`break` can only be used inside a loop",
            ErrorCode::E0508 => "`continue` can only be used inside a loop",
            ErrorCode::E0509 => "required trait method is not implemented",
            ErrorCode::E0510 => "trait impl method return type does not match trait definition",
            ErrorCode::E0512 => "only named function calls are supported",
            ErrorCode::E0513 => "builtin function argument count error",
            ErrorCode::E0514 => "unused return value of pure function",
            ErrorCode::E0515 => "unused variable",
            ErrorCode::E0516 => "compiler recursion limit exceeded",
        }
    }

    /// Parse an error code from a string like "E0001".
    pub fn parse(s: &str) -> Option<ErrorCode> {
        let all = Self::all();
        all.into_iter().find(|code| code.as_str() == s)
    }

    /// Returns all error codes.
    pub fn all() -> Vec<ErrorCode> {
        vec![
            ErrorCode::E0001,
            ErrorCode::E0002,
            ErrorCode::E0003,
            ErrorCode::E0007,
            ErrorCode::E0100,
            ErrorCode::E0101,
            ErrorCode::E0102,
            ErrorCode::E0103,
            ErrorCode::E0104,
            ErrorCode::E0105,
            ErrorCode::E0106,
            ErrorCode::E0107,
            ErrorCode::E0108,
            ErrorCode::E0109,
            ErrorCode::E0110,
            ErrorCode::E0111,
            ErrorCode::E0112,
            ErrorCode::E0113,
            ErrorCode::E0114,
            ErrorCode::E0115,
            ErrorCode::E0116,
            ErrorCode::E0117,
            ErrorCode::E0118,
            ErrorCode::E0119,
            ErrorCode::E0120,
            ErrorCode::E0121,
            ErrorCode::E0122,
            ErrorCode::E0123,
            ErrorCode::E0124,
            ErrorCode::E0125,
            ErrorCode::E0126,
            ErrorCode::E0127,
            ErrorCode::E0128,
            ErrorCode::E0129,
            ErrorCode::E0130,
            ErrorCode::E0131,
            ErrorCode::E0132,
            ErrorCode::E0133,
            ErrorCode::E0134,
            ErrorCode::E0135,
            ErrorCode::E0136,
            ErrorCode::E0137,
            ErrorCode::E0200,
            ErrorCode::E0201,
            ErrorCode::E0202,
            ErrorCode::E0300,
            ErrorCode::E0301,
            ErrorCode::E0302,
            ErrorCode::E0303,
            ErrorCode::E0304,
            ErrorCode::E0305,
            ErrorCode::E0306,
            ErrorCode::E0307,
            ErrorCode::E0308,
            ErrorCode::E0309,
            ErrorCode::E0310,
            ErrorCode::E0311,
            ErrorCode::E0313,
            ErrorCode::E0314,
            ErrorCode::E0315,
            ErrorCode::E0316,
            ErrorCode::E0317,
            ErrorCode::E0318,
            ErrorCode::E0319,
            ErrorCode::E0323,
            ErrorCode::E0324,
            ErrorCode::E0400,
            ErrorCode::E0401,
            ErrorCode::E0402,
            ErrorCode::E0403,
            ErrorCode::E0404,
            ErrorCode::E0405,
            ErrorCode::E0406,
            ErrorCode::E0501,
            ErrorCode::E0502,
            ErrorCode::E0503,
            ErrorCode::E0504,
            ErrorCode::E0505,
            ErrorCode::E0506,
            ErrorCode::E0507,
            ErrorCode::E0508,
            ErrorCode::E0509,
            ErrorCode::E0510,
            ErrorCode::E0512,
            ErrorCode::E0513,
            ErrorCode::E0514,
            ErrorCode::E0515,
            ErrorCode::E0516,
        ]
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
