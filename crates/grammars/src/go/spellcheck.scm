(comment) @spell
(interpreted_string_literal_content) @spell

; Names introduced here; uses are spelled wherever they are defined.
(function_declaration name: (identifier) @spell)
(method_declaration name: (field_identifier) @spell)
(method_elem name: (field_identifier) @spell)
(type_spec name: (type_identifier) @spell)
(field_declaration name: (field_identifier) @spell)
(parameter_declaration name: (identifier) @spell)
(var_spec name: (identifier) @spell)
(const_spec name: (identifier) @spell)
(short_var_declaration left: (expression_list (identifier) @spell))
(range_clause left: (expression_list (identifier) @spell))

(import_spec path: (interpreted_string_literal) @nospell)
