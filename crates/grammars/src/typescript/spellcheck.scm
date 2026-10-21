(comment) @spell
(string_fragment) @spell

; Names introduced here; uses are spelled wherever they are defined.
(function_declaration name: (identifier) @spell)
(generator_function_declaration name: (identifier) @spell)
(class_declaration name: (type_identifier) @spell)
(method_definition name: (property_identifier) @spell)
(public_field_definition name: (property_identifier) @spell)
(variable_declarator name: (identifier) @spell)
(required_parameter pattern: (identifier) @spell)
(optional_parameter pattern: (identifier) @spell)
(catch_clause parameter: (identifier) @spell)
(interface_declaration name: (type_identifier) @spell)
(property_signature name: (property_identifier) @spell)
(type_alias_declaration name: (type_identifier) @spell)
(enum_declaration name: (identifier) @spell)

; Module specifiers
(import_statement source: (string) @nospell)
(export_statement source: (string) @nospell)
(call_expression
  function: [(identifier) (import)] @_fn
  arguments: (arguments . (string) @nospell)
  (#any-of? @_fn "require" "import"))
