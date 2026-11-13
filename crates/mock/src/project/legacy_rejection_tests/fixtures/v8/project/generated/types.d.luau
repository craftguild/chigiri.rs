declare extern type ChigiriNull with
end
-- Executable capability contract: chigiri-mock-capabilities/v2
export type ChigiriOsDateInput = {
    year: number,
    month: number,
    day: number,
    hour: number?,
    min: number?,
    sec: number?,
    isdst: boolean?,
}
export type ChigiriOsDateResult = {
    year: number,
    month: number,
    day: number,
    hour: number,
    min: number,
    sec: number,
    wday: number,
    yday: number,
    isdst: boolean,
}
export type TimeleapOptions = { world: boolean? }
export type ChigiriStringLibrary = {
    ["len"]: (value: string) -> number,
    ["sub"]: (value: string, start: number, finish: number?) -> string,
    ["find"]: (
        value: string,
        pattern: string,
        start: number?,
        plain: boolean?
    ) -> (number?, number?),
    ["lower"]: (value: string) -> string,
    ["upper"]: (value: string) -> string,
}
export type ChigiriMathLibrary = {
    ["abs"]: (value: number) -> number,
    ["ceil"]: (value: number) -> number,
    ["floor"]: (value: number) -> number,
    ["min"]: (first: number, ...number) -> number,
    ["max"]: (first: number, ...number) -> number,
}
export type ChigiriTableLibrary = {
    ["insert"]: (<T>({ T }, value: T) -> ()) & (<T>({ T }, position: number, value: T) -> ()),
    ["remove"]: <T>({ T }, position: number?) -> T?,
    ["sort"]: <T>({ T }, comparator: ((T, T) -> boolean)?) -> (),
    ["concat"]: (
        { string | number },
        separator: string?,
        start: number?,
        finish: number?
    ) -> string,
    ["clone"]: <T>(value: T) -> T,
}
export type ChigiriOsLibrary = {
    ["time"]: (date: ChigiriOsDateInput?) -> number?,
    ["date"]: (() -> string?)
        & ((format: nil, timestamp: number?) -> string?)
        & ((format: "*t" | "!*t", timestamp: number?) -> ChigiriOsDateResult?)
        & ((format: string?, timestamp: number?) -> (string | ChigiriOsDateResult)?),
    ["difftime"]: (first: number, second: number?) -> number,
    ["clock"]: () -> number,
}
export type ChigiriTimeleapLibrary = {
    ["travel_to"]: (timestamp: number, options: TimeleapOptions?) -> (),
    ["reset"]: (options: TimeleapOptions?) -> (),
}
export type ChigiriGlobalLibrary = {
    ["assert"]: <T>(value: T, message: string?) -> T,
    ["error"]: (message: any, level: number?) -> never,
    ["pcall"]: (callback: (...any) -> ...any, ...any) -> (boolean, ...any),
    ["type"]: (value: any) -> string,
    ["pairs"]: <K, V>({ [K]: V }) -> (({ [K]: V }, K?) -> (K?, V), { [K]: V }, nil),
    ["ipairs"]: <V>({ V }) -> (({ V }, number) -> (number?, V), { V }, number),
    ["tostring"]: (value: any) -> string,
    ["tonumber"]: (value: string | number, radix: number?) -> number?,
    ["string"]: ChigiriStringLibrary,
    ["math"]: ChigiriMathLibrary,
    ["table"]: ChigiriTableLibrary,
    ["os"]: ChigiriOsLibrary,
    ["timeleap"]: ChigiriTimeleapLibrary,
}
export type ChigiriNoTimeleapGlobalLibrary = {
    ["assert"]: <T>(value: T, message: string?) -> T,
    ["error"]: (message: any, level: number?) -> never,
    ["pcall"]: (callback: (...any) -> ...any, ...any) -> (boolean, ...any),
    ["type"]: (value: any) -> string,
    ["pairs"]: <K, V>({ [K]: V }) -> (({ [K]: V }, K?) -> (K?, V), { [K]: V }, nil),
    ["ipairs"]: <V>({ V }) -> (({ V }, number) -> (number?, V), { V }, number),
    ["tostring"]: (value: any) -> string,
    ["tonumber"]: (value: string | number, radix: number?) -> number?,
    ["string"]: ChigiriStringLibrary,
    ["math"]: ChigiriMathLibrary,
    ["table"]: ChigiriTableLibrary,
    ["os"]: ChigiriOsLibrary,
}
-- Type aliases preserve SDL names. Table keys preserve normal SDL field names. ChigiriNull distinguishes explicit null from omission.
export type ChigiriJson = ChigiriNull | ChigiriJsonValue
export type ChigiriJsonValue = string | number | boolean | ChigiriJsonObject | ChigiriJsonArray
export type ChigiriJsonObject = { [string]: ChigiriJson }
export type ChigiriJsonArray = { ChigiriJson }
export type ChigiriStateKind =
    { kind: "string" }
    | { kind: "boolean" }
    | { kind: "integer" }
    | { kind: "number" }
    | { kind: "object", fields: { [string]: ChigiriStateKind } }
    | { kind: "list" | "nullable", of: ChigiriStateKind }
export type ChigiriConfigValue =
    string
    | boolean
    | number
    | { [string]: ChigiriConfigValue }
    | { ChigiriConfigValue }
export type ChigiriHookRegistration = { before: boolean, after: boolean }
export type ChigiriClockConfig = { mode: "system" } | { mode: "fixed", now: number }
export type ChigiriMockProjectConfig = {
    format: "chigiri-mock-project/v8",
    schema: { sources: { string } },
    storage: { sources: { string } },
    clock: ChigiriClockConfig,
    server: { listen: string, allowed_origins: { string } },
    hooks: {
        request: ChigiriHookRegistration,
        operation: ChigiriHookRegistration,
        field: ChigiriHookRegistration,
    },
    state: { fields: { [string]: ChigiriStateKind }, initial: { [string]: ChigiriConfigValue } },
}
export type ChigiriIntValue = number
export type ChigiriFloatValue = number
export type ChigiriStringValue = string
export type ChigiriBooleanValue = boolean
export type ChigiriIDValue = string
export type ChigiriTodoStored = {
    ["__typename"]: "Todo"?,
    ["completed"]: ChigiriBooleanValue,
    ["id"]: ChigiriIDValue,
    ["title"]: ChigiriStringValue,
}
export type ChigiriTodoConcreteStored = {
    ["__typename"]: "Todo",
    ["completed"]: ChigiriBooleanValue,
    ["id"]: ChigiriIDValue,
    ["title"]: ChigiriStringValue,
}
export type ChigiriQueryObject = {
    ["__typename"]: "Query"?,
}
export type ChigiriQueryStored = {
    ["__typename"]: "Query"?,
}
export type ChigiriQueryConcreteStored = {
    ["__typename"]: "Query",
}
export type ChigiriMutationObject = {
    ["__typename"]: "Mutation"?,
}
export type ChigiriMutationStored = {
    ["__typename"]: "Mutation"?,
}
export type ChigiriMutationConcreteStored = {
    ["__typename"]: "Mutation",
}
export type ChigiriChigiriMockRepresentationValue =
    "STRING"
    | "BOOLEAN"
    | "INTEGER"
    | "NUMBER"
    | "OBJECT"
    | "ARRAY"
    | "JSON"
export type ChigiriBusinessFailure = { code: string, message: string? }
export type ChigiriTodoRecord = ChigiriTodoStored & {}
export type ChigiriTodoConcreteRecord = ChigiriTodoRecord & {
    ["__typename"]: "Todo",
}
export type ChigiriTodoCreateInput = {
    ["completed"]: ChigiriBooleanValue,
    ["id"]: ChigiriIDValue | nil,
    ["title"]: ChigiriStringValue,
}
export type ChigiriTodoUpdateInput = {
    ["completed"]: ChigiriBooleanValue | ChigiriNull | nil,
    ["title"]: ChigiriStringValue | ChigiriNull | nil,
}
export type ChigiriTodoWhere =
    { ["all"]: { ChigiriTodoWhere } }
    | { ["any"]: { ChigiriTodoWhere } }
    | { ["field"]: "completed", ["op"]: "eq", ["value"]: ChigiriBooleanValue }
    | { ["field"]: "completed", ["op"]: "ne", ["value"]: ChigiriBooleanValue }
    | { ["field"]: "completed", ["op"]: "in", ["value"]: { ChigiriBooleanValue } }
    | { ["field"]: "completed", ["op"]: "isNull", ["value"]: boolean }
    | { ["field"]: "id", ["op"]: "eq", ["value"]: ChigiriIDValue }
    | { ["field"]: "id", ["op"]: "ne", ["value"]: ChigiriIDValue }
    | { ["field"]: "id", ["op"]: "in", ["value"]: { ChigiriIDValue } }
    | { ["field"]: "id", ["op"]: "isNull", ["value"]: boolean }
    | { ["field"]: "title", ["op"]: "eq", ["value"]: ChigiriStringValue }
    | { ["field"]: "title", ["op"]: "ne", ["value"]: ChigiriStringValue }
    | { ["field"]: "title", ["op"]: "in", ["value"]: { ChigiriStringValue } }
    | { ["field"]: "title", ["op"]: "contains", ["value"]: ChigiriStringValue }
    | { ["field"]: "title", ["op"]: "isNull", ["value"]: boolean }
export type ChigiriTodoOrderKey = {
    ["field"]: "completed" | "id" | "title",
    ["direction"]: "asc" | "desc",
}
export type ChigiriTodoFindOptions = {
    ["where"]: (ChigiriTodoWhere | ChigiriNull)?,
    ["orderBy"]: { ChigiriTodoOrderKey }?,
    ["offset"]: number?,
    ["limit"]: number?,
}
export type ChigiriTodoCountOptions = {
    ["where"]: (ChigiriTodoWhere | ChigiriNull)?,
}
export type ChigiriTodoCountMethod = (ChigiriTodoReadModel, ChigiriTodoCountOptions) -> number
export type ChigiriTodoFindMethod = (
    ChigiriTodoReadModel,
    ChigiriTodoFindOptions
) -> { ChigiriTodoConcreteRecord }
export type ChigiriTodoCreateMethod = (
    ChigiriTodoWriteModel,
    ChigiriTodoCreateInput
) -> ChigiriTodoConcreteRecord
export type ChigiriTodoUpdateMethod = (
    ChigiriTodoWriteModel,
    ChigiriTodoFindOptions,
    ChigiriTodoUpdateInput
) -> { ChigiriTodoConcreteRecord }
export type ChigiriTodoDeleteMethod = (ChigiriTodoWriteModel, ChigiriTodoFindOptions) -> number
export type ChigiriTodoReadModel = {
    ["count"]: ChigiriTodoCountMethod,
    ["find"]: ChigiriTodoFindMethod,
}
export type ChigiriTodoWriteModel = {
    ["count"]: ChigiriTodoCountMethod,
    ["find"]: ChigiriTodoFindMethod,
    ["create"]: ChigiriTodoCreateMethod,
    ["update"]: ChigiriTodoUpdateMethod,
    ["delete"]: ChigiriTodoDeleteMethod,
}
export type ChigiriReadContext = {
    ["scope"]: "read",
    ["null"]: ChigiriNull,
    ["state"]: ChigiriState,
    ["request"]: ChigiriHookRequest | ChigiriNull,
    ["fail"]: (failure: ChigiriBusinessFailure) -> never,
    ["jsonObject"]: (ChigiriJsonObject) -> ChigiriJsonObject,
    ["jsonArray"]: (ChigiriJsonArray) -> ChigiriJsonArray,
    ["models"]: {
        ["Todo"]: ChigiriTodoReadModel,
    },
}
export type ChigiriWriteContext = {
    ["scope"]: "mutation",
    ["null"]: ChigiriNull,
    ["state"]: ChigiriState,
    ["request"]: ChigiriHookRequest | ChigiriNull,
    ["fail"]: (failure: ChigiriBusinessFailure) -> never,
    ["jsonObject"]: (ChigiriJsonObject) -> ChigiriJsonObject,
    ["jsonArray"]: (ChigiriJsonArray) -> ChigiriJsonArray,
    ["models"]: {
        ["Todo"]: ChigiriTodoWriteModel,
    },
}
export type ChigiriState = {}
export type ChigiriStatePatch = {}
export type ChigiriHookHeader = {
    ["name"]: string,
    ["value"]: string,
}
export type ChigiriHookRequest = {
    ["method"]: string,
    ["host"]: string,
    ["port"]: number,
    ["url"]: string,
    ["body"]: string,
    ["headers"]: { ChigiriHookHeader },
}
export type ChigiriHookOperation = {
    ["kind"]: "query" | "mutation" | "subscription",
    ["name"]: string | ChigiriNull,
    ["variables"]: ChigiriJsonObject,
}
export type ChigiriHookField = {
    ["coordinate"]: string,
    ["path"]: { string | number },
    ["arguments"]: ChigiriJsonObject,
    ["parent"]: ChigiriJsonObject | ChigiriNull,
}
export type ChigiriHookError = {
    ["message"]: string,
    ["code"]: string,
    ["path"]: { string | number },
}
export type ChigiriHookResponse = {
    ["data"]: ChigiriJson,
    ["errors"]: { ChigiriHookError },
}
export type ChigiriHookFieldOutcome = {
    ["value"]: ChigiriJson,
    ["errors"]: { ChigiriHookError },
}
export type ChigiriHookRejectReason = {
    ["code"]: number,
    ["message"]: string?,
}
export type ChigiriHookLog = (
    level: "debug" | "info" | "warn" | "error",
    message: string,
    fields: ChigiriJsonObject?
) -> ()
export type ChigiriHookReject = (reason: ChigiriHookRejectReason) -> never
export type ChigiriRequestBeforeContext = ChigiriReadContext & {
    ["log"]: ChigiriHookLog,
    ["reject"]: ChigiriHookReject,
    ["request"]: ChigiriHookRequest,
}
export type ChigiriRequestBeforeResult = {
    ["statePatch"]: ChigiriStatePatch,
} | nil
export type ChigiriRequestBeforeHook = {
    ["run"]: (ctx: ChigiriRequestBeforeContext) -> ChigiriRequestBeforeResult,
}
export type ChigiriRequestAfterContext = ChigiriReadContext & {
    ["log"]: ChigiriHookLog,
    ["result"]: ChigiriHookResponse,
}
export type ChigiriRequestAfterResult = nil
export type ChigiriRequestAfterHook = {
    ["run"]: (ctx: ChigiriRequestAfterContext) -> ChigiriRequestAfterResult,
}
export type ChigiriOperationBeforeContext = ChigiriReadContext & {
    ["log"]: ChigiriHookLog,
    ["reject"]: ChigiriHookReject,
    ["request"]: ChigiriHookRequest,
    ["operation"]: ChigiriHookOperation,
}
export type ChigiriOperationBeforeResult = {
    ["statePatch"]: ChigiriStatePatch,
} | nil
export type ChigiriOperationBeforeHook = {
    ["run"]: (ctx: ChigiriOperationBeforeContext) -> ChigiriOperationBeforeResult,
}
export type ChigiriOperationAfterContext = ChigiriReadContext & {
    ["log"]: ChigiriHookLog,
    ["result"]: ChigiriHookResponse,
    ["operation"]: ChigiriHookOperation,
}
export type ChigiriOperationAfterResult = nil
export type ChigiriOperationAfterHook = {
    ["run"]: (ctx: ChigiriOperationAfterContext) -> ChigiriOperationAfterResult,
}
export type ChigiriFieldBeforeContext = ChigiriReadContext & {
    ["log"]: ChigiriHookLog,
    ["reject"]: ChigiriHookReject,
    ["request"]: ChigiriHookRequest,
    ["operation"]: ChigiriHookOperation,
    ["field"]: ChigiriHookField,
}
export type ChigiriFieldBeforeResult = nil
export type ChigiriFieldBeforeHook = {
    ["run"]: (ctx: ChigiriFieldBeforeContext) -> ChigiriFieldBeforeResult,
}
export type ChigiriFieldAfterContext = ChigiriReadContext & {
    ["log"]: ChigiriHookLog,
    ["result"]: ChigiriHookFieldOutcome,
    ["operation"]: ChigiriHookOperation,
    ["field"]: ChigiriHookField,
}
export type ChigiriFieldAfterResult = nil
export type ChigiriFieldAfterHook = {
    ["run"]: (ctx: ChigiriFieldAfterContext) -> ChigiriFieldAfterResult,
}
export type ChigiriQueryTodosArgs = {}
export type ChigiriQueryTodosResult = { ChigiriTodoRecord }
export type ChigiriQueryTodosContext = ChigiriReadContext
export type ChigiriQueryTodosResolver = {
    resolve: (
        ctx: ChigiriQueryTodosContext,
        args: ChigiriQueryTodosArgs
    ) -> ChigiriQueryTodosResult,
}
export type ChigiriMutationAddTodoArgs = {
    ["title"]: ChigiriStringValue,
}
export type ChigiriMutationAddTodoResult = ChigiriTodoRecord
export type ChigiriMutationAddTodoContext = ChigiriWriteContext
export type ChigiriMutationAddTodoResolver = {
    resolve: (
        ctx: ChigiriMutationAddTodoContext,
        args: ChigiriMutationAddTodoArgs
    ) -> ChigiriMutationAddTodoResult,
}
