; Prose
[
  (line_comment)
  (block_comment)
] @spell

(string_content) @spell

; Names introduced here. Uses of names (calls, paths, trait impl methods) are
; skipped: they are spelled wherever they are defined.
(source_file (function_item name: (identifier) @spell))
(mod_item body: (declaration_list (function_item name: (identifier) @spell)))
(block (function_item name: (identifier) @spell))
(impl_item
  !trait
  body: (declaration_list (function_item name: (identifier) @spell)))
(trait_item
  body: (declaration_list [
    (function_item name: (identifier) @spell)
    (function_signature_item name: (identifier) @spell)
  ]))

(parameter pattern: (identifier) @spell)
(let_declaration pattern: (identifier) @spell)
(closure_parameters (identifier) @spell)

(struct_item name: (type_identifier) @spell)
(enum_item name: (type_identifier) @spell)
(union_item name: (type_identifier) @spell)
(trait_item name: (type_identifier) @spell)
(type_item name: (type_identifier) @spell)
(field_declaration name: (field_identifier) @spell)
(enum_variant name: (identifier) @spell)
(const_item name: (identifier) @spell)
(static_item name: (identifier) @spell)
(mod_item name: (identifier) @spell)
