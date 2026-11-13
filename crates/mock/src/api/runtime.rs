//! Executable Luau capabilities shared by the sandbox, checker, and reference.

/// The capability contract version bound into generated Mock artifacts.
pub(crate) const VERSION: &str = "chigiri-mock-capabilities/v4";

/// One standard function exposed by the Mock sandbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FunctionSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub signature: &'static str,
    pub description: &'static str,
}

impl FunctionSpec {
    /// Return the Luau function type encoded by the published signature.
    pub(crate) fn function_type(&self) -> &'static str {
        let generic = self.signature.find('<');
        let arguments = self.signature.find('(');
        let start = match (generic, arguments) {
            (Some(generic), Some(arguments)) => generic.min(arguments),
            (Some(generic), None) => generic,
            (None, Some(arguments)) => arguments,
            (None, None) => panic!("capability signature must contain a function type"),
        };
        &self.signature[start..]
    }

    /// Keep checker intrinsics whose flow refinements cannot be represented by a function type.
    pub(crate) fn checker_intrinsic(&self) -> bool {
        matches!(self.id, "stdlib.assert" | "stdlib.error" | "stdlib.type")
    }
}

/// One closed standard-library table exposed by the Mock sandbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LibrarySpec {
    pub name: &'static str,
    pub type_name: &'static str,
    pub functions: &'static [FunctionSpec],
}

macro_rules! functions {
    ($($id:literal => $name:literal, $signature:literal, $description:literal;)+) => {
        &[$(FunctionSpec { id: $id, name: $name, signature: $signature, description: $description },)+]
    };
}

pub(crate) const GLOBALS: &[FunctionSpec] = functions! {
    "stdlib.assert" => "assert", "assert<T>(value: T, message: string?) -> T", "Require a truthy value and return that value.";
    "stdlib.error" => "error", "error(message: any, level: number?) -> never", "Raise an internal script failure.";
    "stdlib.pcall" => "pcall", "pcall(callback: (...any) -> ...any, ...any) -> (boolean, ...any)", "Run a protected call under the bounded reentry contract.";
    "stdlib.type" => "type", "type(value: any) -> string", "Return the Luau runtime type name.";
    "stdlib.pairs" => "pairs", "pairs<K, V>({[K]: V}) -> (({[K]: V}, K?) -> (K?, V), {[K]: V}, nil)", "Iterate the key/value entries of a table.";
    "stdlib.ipairs" => "ipairs", "ipairs<V>({V}) -> (({V}, number) -> (number?, V), {V}, number)", "Iterate the dense sequence entries of a table.";
    "stdlib.tostring" => "tostring", "tostring(value: any) -> string", "Convert a primitive value to text.";
    "stdlib.tonumber" => "tonumber", "tonumber(value: string | number, radix: number?) -> number?", "Parse a finite numeric value when possible.";
};

const STRING_FUNCTIONS: &[FunctionSpec] = functions! {
    "stdlib.string.len" => "len", "string.len(value: string) -> number", "Return the byte length of a string.";
    "stdlib.string.sub" => "sub", "string.sub(value: string, start: number, finish: number?) -> string", "Return a bounded byte-indexed substring.";
    "stdlib.string.find" => "find", "string.find(value: string, pattern: string, start: number?, plain: boolean?) -> (number?, number?)", "Return the first and last indexes of a plain string or bounded Luau pattern.";
    "stdlib.string.lower" => "lower", "string.lower(value: string) -> string", "Convert ASCII letters to lowercase.";
    "stdlib.string.upper" => "upper", "string.upper(value: string) -> string", "Convert ASCII letters to uppercase.";
};

const MATH_FUNCTIONS: &[FunctionSpec] = functions! {
    "stdlib.math.abs" => "abs", "math.abs(value: number) -> number", "Return an absolute numeric value.";
    "stdlib.math.ceil" => "ceil", "math.ceil(value: number) -> number", "Round a number toward positive infinity.";
    "stdlib.math.floor" => "floor", "math.floor(value: number) -> number", "Round a number toward negative infinity.";
    "stdlib.math.min" => "min", "math.min(first: number, ...number) -> number", "Return the least supplied number.";
    "stdlib.math.max" => "max", "math.max(first: number, ...number) -> number", "Return the greatest supplied number.";
};

const TABLE_FUNCTIONS: &[FunctionSpec] = functions! {
    "stdlib.table.insert" => "insert", "table.insert: (<T>({T}, value: T) -> ()) & (<T>({T}, position: number, value: T) -> ())", "Insert a value into a mutable sequence, optionally at a position.";
    "stdlib.table.remove" => "remove", "table.remove<T>({T}, position: number?) -> T?", "Remove a value from a mutable sequence.";
    "stdlib.table.sort" => "sort", "table.sort<T>({T}, comparator: ((T, T) -> boolean)?) -> ()", "Sort a mutable sequence in place.";
    "stdlib.table.concat" => "concat", "table.concat({string | number}, separator: string?, start: number?, finish: number?) -> string", "Join sequence values into a string.";
    "stdlib.table.clone" => "clone", "table.clone<T>(value: T) -> T", "Create a shallow mutable table copy.";
};

const OS_FUNCTIONS: &[FunctionSpec] = functions! {
    "stdlib.os.time" => "time", "os.time(date: ChigiriOsDateInput?) -> number?", "Read apparent Unix time or convert UTC date fields with the pinned native implementation.";
    "stdlib.os.date" => "date", "os.date: (() -> string?) & ((format: nil, timestamp: number?) -> string?) & ((format: \"*t\" | \"!*t\", timestamp: number?) -> ChigiriOsDateResult?) & ((format: string?, timestamp: number?) -> (string | ChigiriOsDateResult)?)", "Format or decompose a timestamp with apparent time substituted only when timestamp is nil or omitted.";
    "stdlib.os.difftime" => "difftime", "os.difftime(first: number, second: number?) -> number", "Subtract two timestamps in seconds.";
    "stdlib.os.clock" => "clock", "os.clock() -> number", "Read the native monotonic duration clock in live execution phases.";
};

const TIMELEAP_FUNCTIONS: &[FunctionSpec] = functions! {
    "runtime.timeleap.travel_to" => "travel_to", "timeleap.travel_to(timestamp: number, options: TimeleapOptions?) -> ()", "Select an admitted fixed apparent time for an authorized request or host-owned world control.";
    "runtime.timeleap.reset" => "reset", "timeleap.reset(options: TimeleapOptions?) -> ()", "Restore the selected request or world baseline in an authorized control phase.";
};

pub(crate) const LIBRARIES: &[LibrarySpec] = &[
    LibrarySpec {
        name: "string",
        type_name: "ChigiriStringLibrary",
        functions: STRING_FUNCTIONS,
    },
    LibrarySpec {
        name: "math",
        type_name: "ChigiriMathLibrary",
        functions: MATH_FUNCTIONS,
    },
    LibrarySpec {
        name: "table",
        type_name: "ChigiriTableLibrary",
        functions: TABLE_FUNCTIONS,
    },
    LibrarySpec {
        name: "os",
        type_name: "ChigiriOsLibrary",
        functions: OS_FUNCTIONS,
    },
    LibrarySpec {
        name: "timeleap",
        type_name: "ChigiriTimeleapLibrary",
        functions: TIMELEAP_FUNCTIONS,
    },
];

/// Builtins intentionally hidden from both direct lookup and `_G`.
pub(crate) const UNAVAILABLE_GLOBALS: &[&str] = &[
    "bit32",
    "buffer",
    "collectgarbage",
    "coroutine",
    "debug",
    "gcinfo",
    "getfenv",
    "getmetatable",
    "load",
    "loadstring",
    "newproxy",
    "next",
    "print",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "require",
    "select",
    "setfenv",
    "setmetatable",
    "utf8",
    "vector",
    "warn",
    "xpcall",
];

pub(crate) fn paths() -> impl Iterator<Item = String> {
    GLOBALS
        .iter()
        .map(|function| function.name.to_owned())
        .chain(LIBRARIES.iter().flat_map(|library| {
            library
                .functions
                .iter()
                .map(move |function| format!("{}.{}", library.name, function.name))
        }))
}
