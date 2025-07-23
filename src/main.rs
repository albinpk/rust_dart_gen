use glob::glob;
use regex::Regex;
use std::{fs, thread};

const FLU_ANNOTATION: &str = "// @flu";
const CLASS_REGEX: &str = r"^abstract class _(\w+) \{";
const FIELD_REGEX: &str = r"^\s\s([A-Za-z_].*) get (\w+);$";
const GENERIC_LIST_REGEX: &str = r"^List<([A-Za-z_].*)>";

// TODO: deep collection
// TODO: num toInt

fn main() {
    let mut dart_paths: Vec<String> = vec![];
    for entry in glob("lib/**/*.dart").expect("Failed to read glob pattern") {
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

    println!("Found {} dart files", dart_paths.len());

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
        let mut handle = vec![];
        for part in parts {
            handle.push(thread::spawn(move || {
                for path in part {
                    if let Ok(file) = DartFile::from_file(&path) {
                        file.generate_file();
                    }
                }
            }));
        }
        handle.into_iter().for_each(|h| h.join().unwrap());
    } else {
        for path in &dart_paths {
            if let Ok(file) = DartFile::from_file(&path) {
                file.generate_file();
            }
        }
    }

    println!("{} files generated", dart_paths.len());
}

#[derive(Debug)]
struct DartFile {
    path: String,
    classes: Vec<DartClass>,
}
impl DartFile {
    fn new(path: String, classes: Vec<DartClass>) -> Self {
        Self { path, classes }
    }

    fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let content = fs::read_to_string(path)?;
        return Ok(DartFile::from_string(&content, path));
    }

    fn from_string(content: &str, path: &str) -> Self {
        let class_regex = Regex::new(CLASS_REGEX).unwrap();
        let field_regex = Regex::new(FIELD_REGEX).unwrap();

        let lines: Vec<String> = content.lines().map(String::from).collect();
        let mut classes: Vec<DartClass> = vec![];

        let mut annotation_start = false;
        let mut class_start = false;
        let mut depth = 0; // scopes { }

        // parsing all classes and their fields in a single loop
        for line in lines {
            if !annotation_start {
                annotation_start = line == FLU_ANNOTATION;
                continue;
            }

            // removing comment from line
            let line = line.split("//").next().unwrap();
            if line.trim().is_empty() {
                continue;
            }

            // checking for a class declaration
            if !class_start {
                if let Some(cap) = class_regex.captures(line) {
                    // start of a @flu class
                    classes.push(DartClass::new(cap[1].to_string(), false, vec![]));
                    class_start = true;
                    depth = 1;
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
                classes.last_mut().unwrap().fields.push(DartField::new(
                    cap[2].to_string(),
                    DartType::new(cap[1].to_string()),
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
        DartFile::new(path.to_string(), classes)
    }

    fn generated_path(&self) -> String {
        self.path.replace(".dart", ".flu.dart")
    }

    fn file_name(&self) -> String {
        self.path.split("/").last().unwrap().to_string()
        // Path::new(&self.path)
        //     .file_name()
        //     .unwrap()
        //     .to_str()
        //     .unwrap()
        //     .to_string()
    }

    fn generate_file(&self) {
        if self.classes.is_empty() {
            return;
        }
        let mut lines = vec![
            "// dart format off\n".to_string(),
            format!("part of '{}';", self.file_name()),
        ];
        for class in &self.classes {
            // class definition start
            lines.push(format!("\nclass {} extends _{} {{", class.name, class.name));

            Self::add_constructor(class, &mut lines);

            Self::add_fields(class, &mut lines);

            Self::add_from_json(class, &mut lines);

            Self::add_to_json(class, &mut lines);

            Self::add_copy_with(class, &mut lines);

            Self::add_to_string(class, &mut lines);

            Self::add_equal_operator(class, &mut lines);

            Self::add_hash_code(class, &mut lines);

            // class definition end
            lines.push("}".to_string());
        }

        let _ = fs::write(self.generated_path(), lines.join("\n"));
    }

    fn add_constructor(class: &DartClass, lines: &mut Vec<String>) {
        let const_key = if class.has_const_constructor {
            "const "
        } else {
            ""
        };
        lines.push(format!("  {const_key}{}({{", class.name));
        for field in &class.fields {
            lines.push(format!("    required this.{},", field.name));
        }
        lines.push("  });\n".to_string());
    }

    fn add_fields(class: &DartClass, lines: &mut Vec<String>) {
        for field in &class.fields {
            lines.push(format!(
                "  final {} {};",
                field.typ.type_string(),
                field.name
            ));
        }
    }

    fn add_from_json(class: &DartClass, lines: &mut Vec<String>) {
        lines.push(format!(
            "\n  factory {}.fromJson(Map<String, dynamic> json) {{",
            class.name
        ));
        lines.push(format!("    return {}(", class.name));

        for DartField { name, typ } in &class.fields {
            let value = match typ {
                DartType::Concrete(concrete) => concrete.from_json_value(format!("json['{name}']")),
                DartType::GenericList { typ, nullable } => {
                    let mapper = format!("(e) => {}", typ.from_json_value("e".to_string()));
                    let null_mark = if *nullable { "?" } else { "" };
                    format!(
                        "(json['{name}'] as List{}){}.map({mapper}).toList()",
                        null_mark, null_mark
                    )
                }
            };
            lines.push(format!("      {name}: {value},"));
        }
        lines.push("    );".to_string());
        lines.push("  }".to_string());
    }

    fn add_to_json(class: &DartClass, lines: &mut Vec<String>) {
        lines.push("\n  Map<String, dynamic> toJson() => {".to_string());
        for DartField { name, typ } in &class.fields {
            let value = match typ {
                DartType::Concrete(concrete) => concrete.to_json_value(name.to_string()),
                DartType::GenericList { typ, nullable } => {
                    if typ.is_custom() {
                        let mapper = format!("(e) => {}", typ.to_json_value("e".to_string()));
                        let null_mark = if *nullable { "?" } else { "" };
                        format!("{name}{null_mark}.map({mapper}).toList()")
                    } else {
                        name.to_string()
                    }
                }
            };
            lines.push(format!("    '{name}': {value},"));
        }
        lines.push("  };".to_string());
    }

    fn add_copy_with(class: &DartClass, lines: &mut Vec<String>) {
        lines.push(format!("\n  {} copyWith({{", class.name));
        for DartField { name, typ } in &class.fields {
            lines.push(format!("    {}? {name},", typ.non_null_type_string()));
        }
        lines.push(format!("  }}) => {}(", class.name));
        for DartField { name, typ: _ } in &class.fields {
            lines.push(format!("    {name}: {name} ?? this.{name},"));
        }
        lines.push("  );".to_string());
    }

    fn add_to_string(class: &DartClass, lines: &mut Vec<String>) {
        lines.push(format!(
            "\n  @override\n  String toString() => '{}('",
            class.name
        ));
        for DartField { name, typ: _ } in &class.fields {
            lines.push(format!("    '{name}: ${name} '",));
        }
        lines.push("    ')';".to_string());
    }

    fn add_equal_operator(class: &DartClass, lines: &mut Vec<String>) {
        lines.push("\n  @override\n  bool operator ==(Object other) {".to_string());
        lines.push("    if (identical(this, other)) return true;".to_string());
        lines.push(format!("    return other is {}", class.name));
        let mut equals = vec![];
        for DartField { name, typ: _ } in &class.fields {
            equals.push(format!("      && other.{name} == {name}"));
        }
        lines.push(equals.join("\n") + ";");
        lines.push("  }".to_string());
    }

    fn add_hash_code(class: &DartClass, lines: &mut Vec<String>) {
        // hashCode
        lines.push("\n  @override\n  int get hashCode => Object.hashAll([".to_string());
        for DartField { name, typ: _ } in &class.fields {
            lines.push(format!("    {}.hashCode,", name));
        }
        lines.push("  ]);".to_string());
    }
}

#[derive(Debug)]
struct DartClass {
    name: String,
    has_const_constructor: bool,
    fields: Vec<DartField>,
}
impl DartClass {
    fn new(name: String, has_const_constructor: bool, fields: Vec<DartField>) -> Self {
        Self {
            name,
            has_const_constructor,
            fields,
        }
    }
}

#[derive(Debug)]
struct DartField {
    name: String,
    typ: DartType,
}
impl DartField {
    fn new(name: String, typ: DartType) -> Self {
        Self { name, typ }
    }
}

#[derive(Debug)]
enum ConcreteType {
    Int,
    Double,
    Bool,
    String,
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
            "String" => Self::new(ConcreteType::String, nullable),
            custom => Self::new(ConcreteType::Custom(custom.to_string()), nullable),
        }
    }

    fn type_string(&self) -> String {
        let null_mark = if self.nullable { "?" } else { "" };
        return match &self.typ {
            ConcreteType::Int => "int".to_string(),
            ConcreteType::Double => "double".to_string(),
            ConcreteType::Bool => "bool".to_string(),
            ConcreteType::String => "String".to_string(),
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
        let mat = matches!(self.typ, ConcreteType::Custom(_));
        mat
    }

    fn from_json_value(&self, key: String) -> String {
        if self.is_custom() {
            let factory = format!(
                "{}.fromJson({key} as Map<String, dynamic>)",
                self.non_null_type_string()
            );
            if self.nullable {
                return format!("{key} != null ? {factory} : null",);
            }
            return factory;
        }
        format!("{key} as {}", self.type_string())
    }

    fn to_json_value(&self, key: String) -> String {
        if !self.is_custom() {
            return key;
        }
        format!("{key}{}.toJson()", if self.nullable { "?" } else { "" })
    }
}

#[derive(Debug)]
enum DartType {
    Concrete(Concrete),
    GenericList { typ: Concrete, nullable: bool },
}
impl DartType {
    fn new(name: String) -> Self {
        let generic_list_regex = Regex::new(GENERIC_LIST_REGEX).unwrap();
        match generic_list_regex.captures(&name) {
            Some(cap) => Self::GenericList {
                typ: Concrete::from_string(&cap[1]),
                nullable: name.ends_with("?"),
            },
            None => Self::Concrete(Concrete::from_string(&name)),
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
