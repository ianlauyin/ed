(comment) @spell
(string_content) @spell

; Names introduced here; uses are spelled wherever they are defined.
(function_definition name: (identifier) @spell)
(class_definition name: (identifier) @spell)
(assignment left: (identifier) @spell)
(parameters (identifier) @spell)
(typed_parameter (identifier) @spell)
(default_parameter name: (identifier) @spell)
(typed_default_parameter name: (identifier) @spell)
(lambda_parameters (identifier) @spell)
(aliased_import alias: (identifier) @spell)

; Forward references in annotations belong to the type checker.
(type (string) @nospell)
