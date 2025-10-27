use clap::Parser;
use core::panic;
use glob::glob;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{clone, collections::BTreeMap, fs, path::Path, sync::Arc, thread};

const FLU_ANNOTATION: &str = "// @flu";
const FLU_ANNOTATION_REGEX: &str = r"^// @flu(?: (.*))?$";
const CLASS_REGEX: &str = r"^abstract class _(\w+) \{";
const FIELD_REGEX: &str = r"^\s\s([A-Za-z_].*) get (\w+);$";
const FIELD_ANNOTATION_REGEX: &str = r#"^  // @flu (.*)$"#;
// const FIELD_OPTIONS_REGEX: &str = r#"(?P<key>\w+)(?:=(?P<value>"[^"]+"|\S+))?"#;
const FIELD_OPTIONS_REGEX: &str = r#"(?P<key>\w+)(?:=(?P<value>"[^"]+"|\S+))?|(?P<hash>#\w+)"#;
const GENERIC_LIST_REGEX: &str = r"^List<([A-Za-z_].*)>";

// TODO: deep collection

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to dart files
    #[arg(short, long, default_value = "lib/**/*.dart")]
    path: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
struct RootConfig {
    #[serde(default = "default_flu_config")]
    flu: FluConfig,
}
impl RootConfig {
    fn parse() -> Self {
        let default = RootConfig {
            flu: FluConfig::new(GenConfig::new(true, BTreeMap::new())),
        };
        let x = fs::read_to_string("pub.yaml").unwrap_or("flu:".to_string());

        return match serde_yaml::from_str::<RootConfig>(&x) {
            Ok(c) => c,
            Err(e) => panic!("Failed to parse config file: {e}"),
        };

        match fs::read_to_string("pub.yaml") {
            Ok(content) => {
                println!("Parsing config file");
                // serde_yaml::from_str::<RootConfig>(&content).unwrap_or(default)
                match serde_yaml::from_str::<RootConfig>(&content) {
                    Ok(c) => c,
                    Err(_) => panic!("Failed to parse config file"),
                }
            }
            Err(_) => default,
        }
    }
}

fn default_flu_config() -> FluConfig {
    FluConfig::new(default_gen_config())
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
struct FluConfig {
    #[serde(rename = "gen", default = "default_gen_config")]
    gn: GenConfig,
    // annotations: BTreeMap<String, String>,
}
impl FluConfig {
    fn new(gn: GenConfig) -> Self {
        Self { gn }
    }
}

fn default_gen_config() -> GenConfig {
    GenConfig::new(true, default_annotations())
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
struct GenConfig {
    #[serde(rename = "copyWith", default = "default_true")]
    copy_with: bool,
    #[serde(default = "default_annotations")]
    annotations: BTreeMap<String, String>,
}
impl GenConfig {
    fn new(copy_with: bool, annotations: BTreeMap<String, String>) -> Self {
        Self {
            copy_with,
            annotations,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_annotations() -> BTreeMap<String, String> {
    BTreeMap::new()
}

fn main() {
    let args = Args::parse();

    let mut dart_paths: Vec<String> = vec![];
    for entry in glob(&args.path).expect("Failed to read glob pattern") {
        match entry {
            Err(_) => (),
            Ok(path) => {
                let p = path.to_string_lossy().to_string();
                if !p.ends_with(".flu.dart")
                    && !p.ends_with(".g.dart")
                    && !p.ends_with(".freezed.dart")
                {
                    dart_paths.push(p);
                }
            }
        }
    }

    // println!("{dart_paths:?}");

    // true == enable multi-threading
    if true {
        // TODO: thread count?
        // let x = available_parallelism().unwrap().get();
        // println!("Available parallelism: {x}");
        let parts: Vec<Vec<String>> = dart_paths
            // .chunks(dart_paths.len() / x) // based on cpu
            .chunks(1) // max threads
            .map(|e| e.to_vec())
            .collect();

        let root_config = Arc::new(RootConfig::parse());
        println!("Using config: {:?}", root_config.flu);

        let mut handle = vec![];
        for part in parts {
            let config = Arc::clone(&root_config);
            handle.push(thread::spawn(move || {
                for path in part {
                    if let Ok(file) = DartFile::from_file(&path, &*config) {
                        file.generate_file();
                    }
                }
            }));
        }
        handle.into_iter().for_each(|h| h.join().unwrap());
    } else {
        for path in &dart_paths {
            // if let Ok(file) = DartFile::from_file(&path, root_config_copy) {
            //     println!("gen...");
            //     file.generate_file();
            // }
        }
    }
}

#[derive(Debug)]
struct DartFile {
    path: String,
    classes: Vec<DartClass>,
    config: RootConfig,
}
impl DartFile {
    fn new(path: String, classes: Vec<DartClass>, config: RootConfig) -> Self {
        Self {
            path,
            classes,
            config,
        }
    }

    fn from_file(path: &str, config: &RootConfig) -> Result<Self, std::io::Error> {
        let content = fs::read_to_string(path)?;
        return Ok(DartFile::from_string(&content, path, config));
    }

    fn from_string(content: &str, path: &str, config: &RootConfig) -> Self {
        let flu_annotation_regex = Regex::new(FLU_ANNOTATION_REGEX).unwrap();
        let class_regex = Regex::new(CLASS_REGEX).unwrap();
        let field_regex = Regex::new(FIELD_REGEX).unwrap();
        let field_comment_regex = Regex::new(FIELD_ANNOTATION_REGEX).unwrap();

        let lines: Vec<String> = content.lines().map(String::from).collect();
        let mut class_option: Option<ClassOptions> = None;
        let mut classes: Vec<DartClass> = vec![];

        let mut annotation_start = false;
        let mut class_start = false;
        let mut depth = 0; // scopes { }

        // parsing all classes and their fields in a single loop
        for (i, line) in lines.iter().enumerate() {
            if !annotation_start {
                if let Some(cap) = flu_annotation_regex.captures(line) {
                    if let Some(v) = &cap.get(1) {
                        class_option = Some(ClassOptions::from_string(v.as_str(), config));
                    }
                    annotation_start = true;
                    continue;
                }
            }

            // removing comment from line
            let line = if !line.trim_start().starts_with("// @flu: ") {
                line.split("//").next().unwrap().trim_end()
            } else {
                continue;
            };
            if line.trim_start().is_empty() {
                continue;
            }

            // checking for a class declaration
            if !class_start {
                if let Some(cap) = class_regex.captures(line) {
                    // start of a @flu class
                    classes.push(DartClass::new(
                        cap[1].to_string(),
                        class_option.clone(),
                        false,
                        vec![],
                    ));
                    class_option = None;
                    if !line.ends_with("}") {
                        class_start = true;
                        depth = 1;
                    }
                }
                continue;
            }

            // checking for const constructor
            if depth == 1
                && line == format!("  const _{}();", classes.last().unwrap().name.to_string())
            {
                classes.last_mut().unwrap().has_const_constructor = true;
                continue;
            }

            // checking fields inside class
            if depth == 1
                && let Some(cap) = field_regex.captures(line)
            {
                // checking for field options above the field
                let options = match field_comment_regex.captures(&lines[i - 1]) {
                    Some(cap) => Some(FieldOptions::from_string(&cap[1])),
                    None => None,
                };
                classes.last_mut().unwrap().fields.push(DartField::new(
                    cap[2].to_string(),
                    DartType::from_string_and_options(cap[1].to_string(), &options),
                    options,
                ));
            } else {
                // for skipping method declarations
                if line.contains("{") {
                    depth += 1;
                }
                if line.contains("}") {
                    depth -= 1
                };
                if depth == 0 && class_start {
                    // end of a @flu class
                    class_start = false;
                    annotation_start = false;
                }
            }
        }
        DartFile::new(path.to_string(), classes, config.clone())
    }

    fn generated_path(&self) -> String {
        self.path.replace(".dart", ".flu.dart")
    }

    fn file_name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    fn generate_file(&self) {
        if self.classes.is_empty() {
            return;
        }
        let ignores = "// ignore_for_file: avoid_equals_and_hash_code_on_mutable_classes, document_ignores, lines_longer_than_80_chars";
        let mut lines = vec![
            "// dart format off\n".to_string(),
            ignores.to_string(),
            format!("\npart of '{}';", self.file_name()),
        ];
        for class in &self.classes {
            // gathering class options
            let copy_with = class
                .options
                .as_ref()
                .and_then(|o| o.copy_with)
                .unwrap_or(self.config.flu.gn.copy_with);

            // class definition start
            lines.push(format!("\nclass {} extends _{} {{", class.name, class.name));

            Self::add_constructor(class, &mut lines);

            Self::add_from_json(class, &mut lines);

            Self::add_fields(class, &mut lines);

            Self::add_to_json(class, &mut lines);

            if copy_with {
                Self::add_copy_with(class, &mut lines)
            }

            Self::add_to_string(class, &mut lines);

            Self::add_equal_operator(class, &mut lines);

            Self::add_hash_code(class, &mut lines);

            // class definition end
            lines.push("}\n".to_string());
        }

        let _ = fs::write(self.generated_path(), lines.join("\n"));
    }

    fn add_constructor(class: &DartClass, lines: &mut Vec<String>) {
        let const_key = if class.has_const_constructor {
            "const "
        } else {
            ""
        };
        if class.fields.is_empty() {
            lines.push(format!("  {const_key}{}();", class.name));
        } else {
            lines.push(format!("  {const_key}{}({{", class.name));
            for field in &class.fields {
                lines.push(format!("    required this.{},", field.name));
            }
            lines.push("  });".to_string());
        }
    }

    fn add_from_json(class: &DartClass, lines: &mut Vec<String>) {
        lines.push(format!(
            "\n  factory {}.fromJson(Map<String, dynamic> json) {{",
            class.name
        ));
        lines.push(format!("    return {}(", class.name));
        for field in &class.fields {
            let DartField { name, typ, options } = field;
            let key = field.json_key();
            let value = match typ {
                DartType::Concrete(concrete) => {
                    concrete.from_json_value(format!("json['{key}']"), options)
                }
                DartType::GenericList { typ, nullable } => {
                    let mapper =
                        format!("(e) => {}", typ.from_json_value("e".to_string(), options));
                    let null_mark = if *nullable { "?" } else { "" };
                    format!(
                        "(json['{key}'] as List{}){}.map({mapper}).toList()",
                        null_mark, null_mark
                    )
                }
            };
            lines.push(format!("      {name}: {value},"));
        }
        lines.push("    );".to_string());
        lines.push("  }".to_string());
    }

    fn add_fields(class: &DartClass, lines: &mut Vec<String>) {
        for field in &class.fields {
            lines.push(format!(
                "\n  @override\n  final {} {};",
                field.typ.type_string(),
                field.name
            ));
        }
    }

    fn add_to_json(class: &DartClass, lines: &mut Vec<String>) {
        lines.push("\n  Map<String, dynamic> toJson() => {".to_string());
        for field in &class.fields {
            let DartField { name, typ, options } = field;
            let key = field.json_key();
            let value = match typ {
                DartType::Concrete(concrete) => concrete.to_json_value(name.to_string(), options),
                DartType::GenericList { typ, nullable } => {
                    if typ.is_custom()
                        || matches!(typ.typ, ConcreteType::DateTime | ConcreteType::Enum(_))
                    {
                        let mapper =
                            format!("(e) => {}", typ.to_json_value("e".to_string(), options));
                        let null_mark = if *nullable { "?" } else { "" };
                        format!("{name}{null_mark}.map({mapper}).toList()")
                    } else {
                        name.to_string()
                    }
                }
            };
            lines.push(format!("    '{key}': {value},"));
        }
        lines.push("  };".to_string());
    }

    fn add_copy_with(class: &DartClass, lines: &mut Vec<String>) {
        if class.fields.is_empty() {
            lines.push(format!(
                "\n  {} copyWith() => {}();",
                class.name, class.name
            ));
        } else {
            lines.push(format!("\n  {} copyWith({{", class.name));
            for DartField { name, typ, .. } in &class.fields {
                // no ? for dynamic
                let null_mark = if matches!(
                    typ,
                    DartType::Concrete(Concrete {
                        typ: ConcreteType::Dynamic,
                        ..
                    })
                ) {
                    ""
                } else {
                    "?"
                };
                lines.push(format!(
                    "    {}{null_mark} {name},",
                    typ.non_null_type_string()
                ));
            }
            lines.push(format!("  }}) => {}(", class.name));
            for DartField { name, .. } in &class.fields {
                lines.push(format!("    {name}: {name} ?? this.{name},"));
            }
            lines.push("  );".to_string());
        }
    }

    fn add_to_string(class: &DartClass, lines: &mut Vec<String>) {
        lines.push(format!(
            "\n  @override\n  String toString() => '{}('",
            class.name
        ));
        for DartField { name, .. } in &class.fields {
            lines.push(format!("    '{name}: ${name} '",));
        }
        lines.push("    ')';".to_string());
    }

    fn add_equal_operator(class: &DartClass, lines: &mut Vec<String>) {
        lines.push("\n  @override\n  bool operator ==(Object other) {".to_string());
        lines.push("    if (identical(this, other)) return true;".to_string());
        if class.fields.is_empty() {
            lines.push(format!(
                "    return other is {} && other == this;",
                class.name
            ));
        } else {
            lines.push(format!("    return other is {}", class.name));
            let mut equals = vec![];
            for DartField { name, .. } in &class.fields {
                equals.push(format!("      && other.{name} == {name}"));
            }
            lines.push(equals.join("\n") + ";");
        }
        lines.push("  }".to_string());
    }

    fn add_hash_code(class: &DartClass, lines: &mut Vec<String>) {
        lines.push("\n  @override".to_string());
        if class.fields.is_empty() {
            lines.push("  int get hashCode => super.hashCode;".to_string());
        } else {
            lines.push("  int get hashCode => Object.hashAll([".to_string());
            for DartField { name, .. } in &class.fields {
                lines.push(format!("    {}.hashCode,", name));
            }
            lines.push("  ]);".to_string());
        }
    }
}

#[derive(Debug)]
struct DartClass {
    name: String,
    options: Option<ClassOptions>,
    has_const_constructor: bool,
    fields: Vec<DartField>,
}
impl DartClass {
    fn new(
        name: String,
        options: Option<ClassOptions>,
        has_const_constructor: bool,
        fields: Vec<DartField>,
    ) -> Self {
        Self {
            name,
            options,
            has_const_constructor,
            fields,
        }
    }
}

#[derive(Debug)]
struct DartField {
    name: String,
    typ: DartType,
    options: Option<FieldOptions>,
}
impl DartField {
    fn new(name: String, typ: DartType, options: Option<FieldOptions>) -> Self {
        Self { name, typ, options }
    }

    fn json_key(&self) -> String {
        match &self.options {
            Some(o) => match &o.key {
                Some(k) => k.clone(),
                None => self.name.clone(),
            },
            None => self.name.clone(),
        }
    }
}

#[derive(Debug)]
enum ConcreteType {
    Int,
    Double,
    Bool,
    String,
    Dynamic,
    Enum(String),
    DateTime,
    Custom(String),
}

#[derive(Debug)]
struct Concrete {
    typ: ConcreteType,
    nullable: bool,
}
impl Concrete {
    fn new(typ: ConcreteType, nullable: bool) -> Self {
        Self { typ, nullable }
    }

    fn from_string(name: &str) -> Self {
        let nullable = name.ends_with('?');
        match name.replace("?", "").as_str() {
            "int" => Self::new(ConcreteType::Int, nullable),
            "double" => Self::new(ConcreteType::Double, nullable),
            "bool" => Self::new(ConcreteType::Bool, nullable),
            "dynamic" => Self::new(ConcreteType::Dynamic, false),
            "String" => Self::new(ConcreteType::String, nullable),
            "DateTime" => Self::new(ConcreteType::DateTime, nullable),
            custom => Self::new(ConcreteType::Custom(custom.to_string()), nullable),
        }
    }

    fn type_string(&self) -> String {
        let null_mark = if self.nullable && !matches!(self.typ, ConcreteType::Dynamic) {
            "?"
        } else {
            ""
        };
        return match &self.typ {
            ConcreteType::Int => "int".to_string(),
            ConcreteType::Double => "double".to_string(),
            ConcreteType::Bool => "bool".to_string(),
            ConcreteType::String => "String".to_string(),
            ConcreteType::Dynamic => "dynamic".to_string(),
            ConcreteType::Enum(name) => name.to_string(),
            ConcreteType::DateTime => "DateTime".to_string(),
            ConcreteType::Custom(name) => name.clone(),
        } + null_mark;
    }

    fn non_null_type_string(&self) -> String {
        if self.nullable {
            self.type_string().replace("?", "")
        } else {
            self.type_string()
        }
    }

    fn is_custom(&self) -> bool {
        matches!(self.typ, ConcreteType::Custom(_))
    }

    fn from_json_value(&self, key: String, options: &Option<FieldOptions>) -> String {
        if self.is_custom() {
            let factory = format!(
                "{}.fromJson({key} as Map<String, dynamic>)",
                self.non_null_type_string()
            );
            if self.nullable {
                return format!("{key} == null ? null : {factory}");
            }
            return factory;
        }

        let null_mark = if self.nullable { "?" } else { "" };
        match &self.typ {
            ConcreteType::Int => format!("({key} as num{null_mark}){null_mark}.toInt()"),
            ConcreteType::Double => format!("({key} as num{null_mark}){null_mark}.toDouble()"),
            ConcreteType::Enum(name) => {
                let field = match options {
                    Some(o) => match &o.enum_value {
                        Some(f) => f,
                        None => "name",
                    },
                    None => "name",
                };
                format!(
                    "{}{name}.values.singleWhere((v) => v.{field} == {key} as String)",
                    if self.nullable {
                        format!("{key} == null ? null : ")
                    } else {
                        "".to_string()
                    }
                )
            }
            ConcreteType::Dynamic => key,
            ConcreteType::DateTime => format!(
                "{}DateTime.parse({key} as String)",
                if self.nullable {
                    format!("{key} == null ? null : ")
                } else {
                    "".to_string()
                }
            ),
            ConcreteType::Bool | ConcreteType::String | ConcreteType::Custom(_) => {
                format!("{key} as {}", self.type_string())
            }
        }
    }

    fn to_json_value(&self, key: String, options: &Option<FieldOptions>) -> String {
        let null_mark = if self.nullable { "?" } else { "" };
        match self.typ {
            ConcreteType::Int
            | ConcreteType::Double
            | ConcreteType::Bool
            | ConcreteType::Dynamic
            | ConcreteType::String => key,
            ConcreteType::Enum(_) => {
                let field = match options {
                    Some(o) => match &o.enum_value {
                        Some(f) => f,
                        None => "name",
                    },
                    None => "name",
                };
                format!("{key}{null_mark}.{field}")
            }
            ConcreteType::DateTime => format!("{key}{null_mark}.toIso8601String()"),
            ConcreteType::Custom(_) => format!("{key}{null_mark}.toJson()"),
        }
    }
}

#[derive(Debug)]
enum DartType {
    Concrete(Concrete),
    GenericList { typ: Concrete, nullable: bool },
}
impl DartType {
    fn from_string_and_options(name: String, options: &Option<FieldOptions>) -> Self {
        let generic_list_regex = Regex::new(GENERIC_LIST_REGEX).unwrap();
        let nullable = name.ends_with('?');
        match generic_list_regex.captures(&name) {
            Some(cap) => Self::GenericList {
                typ: match options {
                    Some(o) => match o.is_enum {
                        true => Concrete::new(
                            ConcreteType::Enum(cap[1].replace("?", "")),
                            cap[1].ends_with("?"),
                        ),
                        false => Concrete::from_string(&cap[1]),
                    },
                    None => Concrete::from_string(&cap[1]),
                },
                nullable,
            },
            None => match options {
                Some(o) => match o.is_enum {
                    true => Self::Concrete(Concrete::new(
                        ConcreteType::Enum(name.replace("?", "")),
                        nullable,
                    )),
                    false => Self::Concrete(Concrete::from_string(&name)),
                },
                None => Self::Concrete(Concrete::from_string(&name)),
            },
        }
    }

    fn type_string(&self) -> String {
        match self {
            DartType::Concrete(concrete) => concrete.type_string(),
            DartType::GenericList { typ, nullable } => {
                format!(
                    "List<{}>{}",
                    typ.type_string(),
                    if *nullable { "?" } else { "" }
                )
            }
        }
    }

    fn non_null_type_string(&self) -> String {
        match self {
            DartType::Concrete(concrete) => concrete.non_null_type_string(),
            DartType::GenericList { typ, nullable: _ } => format!("List<{}>", typ.type_string()),
        }
    }
}

#[derive(Debug, Clone)]
struct FieldOptions {
    key: Option<String>,
    is_enum: bool,
    enum_value: Option<String>,
}
impl FieldOptions {
    fn new(key: Option<String>, is_enum: bool, enum_value: Option<String>) -> Self {
        Self {
            key,
            is_enum,
            enum_value,
        }
    }

    fn from_string(value: &str) -> Self {
        let field_option_regex = Regex::new(FIELD_OPTIONS_REGEX).unwrap();
        let mut key: Option<String> = None;
        let mut is_enum = false;
        let mut enum_value: Option<String> = None;
        for cap in field_option_regex.captures_iter(value) {
            if let (Some(k), v) = (cap.name("key"), cap.name("value")) {
                match v {
                    // key=value pair
                    Some(v) => {
                        // println!("{}: {}", k.as_str(), v.as_str());
                        let mut value = v.as_str();
                        if value.starts_with('"') && value.ends_with('"') {
                            value = &value[1..value.len() - 1];
                        }
                        match k.as_str() {
                            "key" => key = Some(value.to_string()),
                            "enumValue" => enum_value = Some(value.to_string()),
                            _ => {}
                        }
                    }
                    None => match k.as_str() {
                        "enum" => is_enum = true,
                        _ => {}
                    },
                }
            }
        }
        return FieldOptions::new(key, is_enum, enum_value);
    }
}

#[derive(Debug, Clone)]
struct ClassOptions {
    copy_with: Option<bool>,
}
impl ClassOptions {
    fn new(copy_with: Option<bool>) -> Self {
        Self { copy_with }
    }

    fn from_string(value: &str, config: &RootConfig) -> Self {
        let field_option_regex = Regex::new(FIELD_OPTIONS_REGEX).unwrap();
        let mut copy_with: Option<bool> = None;
        let mut is_enum = false;
        for cap in field_option_regex.captures_iter(value) {
            if let (k, v, h) = (cap.name("key"), cap.name("value"), cap.name("hash")) {
                if let Some(k) = k {
                    match v {
                        // key=value pair
                        Some(v) => {
                            println!("{}: {}", k.as_str(), v.as_str());
                            let mut value = v.as_str();
                            if value.starts_with('"') && value.ends_with('"') {
                                value = &value[1..value.len() - 1];
                            }
                            match k.as_str() {
                                "copyWith" => {
                                    copy_with = match value {
                                        "true" => Some(true),
                                        "false" => Some(false),
                                        _ => None,
                                    }
                                }
                                _ => {}
                            }
                        }
                        None => {
                            println!("{}", k.as_str());
                            match k.as_str() {
                                "enum" => is_enum = true,
                                _ => {}
                            }
                        }
                    }
                }

                if let Some(h) = h {
                    println!("hash: {}", h.as_str());
                }
            } else {
                println!("no match");
            }
        }
        return ClassOptions::new(copy_with);
    }
}
