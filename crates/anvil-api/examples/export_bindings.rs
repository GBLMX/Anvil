use std::{
    collections::BTreeMap,
    error::Error,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anvil_api::{
    API_VERSION, ApiError, ApplyEditRequest, ApplyEditResponse, CopyRequest, DocumentEdit,
    DocumentSnapshot, EditorView, EmptyResponse, EventSubscribeRequest, EventSubscribeResponse,
    ExecRequest, ExecResponse, FetchRequest, FetchResponse, METHODS, MethodType,
    NotificationSeverity, NotifyRequest, PathRequest, ReadFileRequest, ReadFileResponse,
    RegisterCommandRequest, RegisterCommandResponse, RenameRequest, SetModeRequest,
    SetModeResponse, StatusRequest, TextRange,
};
use schemars::schema_for;
use serde_json::Value;

fn write_json(path: &Path, schema: &Value) -> Result<(), Box<dyn Error>> {
    fs::write(path, serde_json::to_string_pretty(schema)?)?;
    Ok(())
}

fn lua_type(schema: &Value) -> String {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return reference.rsplit('/').next().unwrap_or("any").to_owned();
    }
    if let Some(constant) = schema.get("const").and_then(Value::as_str) {
        return format!("\"{constant}\"");
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => schema
            .get("items")
            .map(lua_type)
            .map(|item| format!("{item}[]"))
            .unwrap_or_else(|| "any[]".to_owned()),
        Some("integer") | Some("number") => "integer".to_owned(),
        Some("string") => "string".to_owned(),
        Some("boolean") => "boolean".to_owned(),
        Some("object") => "table".to_owned(),
        _ => schema
            .get("oneOf")
            .and_then(Value::as_array)
            .map(|variants| {
                variants
                    .iter()
                    .map(lua_type)
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_else(|| "any".to_owned()),
    }
}

fn lua_for_schema(name: &str, schema: &Value) -> String {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let mut output = format!("---@alias {name}\n");
        for value in values.iter().filter_map(Value::as_str) {
            output.push_str(&format!("---| \"{value}\"\n"));
        }
        return output;
    }

    let variants = schema.get("oneOf").and_then(Value::as_array);
    if let Some(variants) = variants {
        let mut output = format!("---@alias {name}\n");
        for variant in variants {
            output.push_str(&format!("---| {}\n", lua_type(variant)));
        }
        return output;
    }

    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return String::new();
    };
    let mut output = format!("---@class {name}\n");
    for (field, field_schema) in properties {
        output.push_str(&format!("---@field {field} {}\n", lua_type(field_schema)));
    }
    output
}

fn method_type_name(value: MethodType) -> String {
    match value {
        MethodType::Empty => "empty".to_owned(),
        MethodType::Named(name) => name.to_owned(),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("bindings");
    fs::create_dir_all(&directory)?;

    let roots = [
        ("EditorView", schema_for!(EditorView)),
        ("DocumentSnapshot", schema_for!(DocumentSnapshot)),
        ("ApplyEditRequest", schema_for!(ApplyEditRequest)),
        ("ApplyEditResponse", schema_for!(ApplyEditResponse)),
        ("TextRange", schema_for!(TextRange)),
        ("DocumentEdit", schema_for!(DocumentEdit)),
        ("SetModeRequest", schema_for!(SetModeRequest)),
        ("SetModeResponse", schema_for!(SetModeResponse)),
        (
            "RegisterCommandRequest",
            schema_for!(RegisterCommandRequest),
        ),
        (
            "RegisterCommandResponse",
            schema_for!(RegisterCommandResponse),
        ),
        ("NotificationSeverity", schema_for!(NotificationSeverity)),
        ("NotifyRequest", schema_for!(NotifyRequest)),
        ("StatusRequest", schema_for!(StatusRequest)),
        ("EventSubscribeRequest", schema_for!(EventSubscribeRequest)),
        (
            "EventSubscribeResponse",
            schema_for!(EventSubscribeResponse),
        ),
        ("PathRequest", schema_for!(PathRequest)),
        ("ReadFileRequest", schema_for!(ReadFileRequest)),
        ("ReadFileResponse", schema_for!(ReadFileResponse)),
        ("RenameRequest", schema_for!(RenameRequest)),
        ("CopyRequest", schema_for!(CopyRequest)),
        ("FetchRequest", schema_for!(FetchRequest)),
        ("FetchResponse", schema_for!(FetchResponse)),
        ("ExecRequest", schema_for!(ExecRequest)),
        ("ExecResponse", schema_for!(ExecResponse)),
        ("EmptyResponse", schema_for!(EmptyResponse)),
        ("ApiError", schema_for!(ApiError)),
    ];

    let mut definitions = BTreeMap::new();
    for (_, schema) in &roots {
        let value = serde_json::to_value(schema)?;
        if let Some(definitions_value) = value.get("$defs").and_then(Value::as_object) {
            for (name, definition) in definitions_value {
                definitions.insert(name.clone(), definition.clone());
            }
        }
    }

    let mut schemas = BTreeMap::new();
    for (name, schema) in roots {
        schemas.insert(name.to_owned(), serde_json::to_value(schema)?);
    }

    for (name, schema) in &schemas {
        let file_name = match name.as_str() {
            "EditorView" => "editor-view.schema.json",
            "DocumentSnapshot" => "document-snapshot.schema.json",
            "ApplyEditRequest" => "apply-edit-request.schema.json",
            "ApplyEditResponse" => "apply-edit-response.schema.json",
            "TextRange" => "text-range.schema.json",
            "DocumentEdit" => "document-edit.schema.json",
            "SetModeRequest" => "set-mode-request.schema.json",
            "SetModeResponse" => "set-mode-response.schema.json",
            "RegisterCommandRequest" => "register-command-request.schema.json",
            "RegisterCommandResponse" => "register-command-response.schema.json",
            "NotificationSeverity" => "notification-severity.schema.json",
            "NotifyRequest" => "notify-request.schema.json",
            "StatusRequest" => "status-request.schema.json",
            "EventSubscribeRequest" => "event-subscribe-request.schema.json",
            "EventSubscribeResponse" => "event-subscribe-response.schema.json",
            "PathRequest" => "path-request.schema.json",
            "ReadFileRequest" => "read-file-request.schema.json",
            "ReadFileResponse" => "read-file-response.schema.json",
            "RenameRequest" => "rename-request.schema.json",
            "CopyRequest" => "copy-request.schema.json",
            "FetchRequest" => "fetch-request.schema.json",
            "FetchResponse" => "fetch-response.schema.json",
            "ExecRequest" => "exec-request.schema.json",
            "ExecResponse" => "exec-response.schema.json",
            "EmptyResponse" => "empty-response.schema.json",
            "ApiError" => "api-error.schema.json",
            other => unreachable!("unhandled root {other}"),
        };
        write_json(&directory.join(file_name), schema)?;
    }

    let mut lua = format!(
        "-- This file was generated by anvil-api/examples/export_bindings.rs.\n\
         -- Do not edit manually.\n\
         -- API version: {API_VERSION}\n\n"
    );
    for (name, schema) in &schemas {
        lua.push_str(&lua_for_schema(name, schema));
        lua.push('\n');
    }
    for (name, schema) in &definitions {
        if schemas.contains_key(name) {
            continue;
        }
        lua.push_str(&lua_for_schema(name, schema));
        lua.push('\n');
    }
    fs::write(directory.join("anvil-api.lua"), lua)?;
    fs::write(
        directory.join("api-version.ts"),
        format!("export const API_VERSION = \"{API_VERSION}\";\n"),
    )?;

    let mut methods_json = Vec::new();
    let mut methods_ts = "// This file was generated by anvil-api/examples/export_bindings.rs.\n\
         // Do not edit manually.\n\
         export const METHODS = {\n"
        .to_owned();
    let mut methods_lua = "-- This file was generated by anvil-api/examples/export_bindings.rs.\n\
         -- Do not edit manually.\n\
         local methods = {\n"
        .to_owned();
    for spec in METHODS {
        let params_name = method_type_name(spec.params);
        let result_name = method_type_name(spec.result);
        methods_json.push(serde_json::json!({
            "name": spec.name,
            "params": params_name,
            "result": result_name,
        }));
        let params = serde_json::to_string(&params_name)?;
        let result = serde_json::to_string(&result_name)?;
        writeln!(
            methods_ts,
            "  \"{}\": {{ params: {}, result: {} }},",
            spec.name, params, result
        )?;
        writeln!(
            methods_lua,
            "  [\"{}\"] = {{ params = \"{}\", result = \"{}\" }},",
            spec.name, params_name, result_name
        )?;
    }
    methods_ts.push_str("} as const;\n");
    methods_lua.push_str("}\n\nreturn methods\n");
    write_json(&directory.join("methods.json"), &Value::Array(methods_json))?;
    fs::write(directory.join("methods.ts"), methods_ts)?;
    fs::write(directory.join("methods.lua"), methods_lua)?;

    // Remove the pre-split schema name if it is still present.
    let old_schema: PathBuf = directory.join("set-mode.schema.json");
    if old_schema.exists() {
        fs::remove_file(old_schema)?;
    }

    Ok(())
}
