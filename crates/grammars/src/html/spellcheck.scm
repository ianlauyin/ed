(comment) @spell
(text) @spell

(attribute
  (attribute_name) @_name
  (quoted_attribute_value (attribute_value) @spell)
  (#any-of? @_name "alt" "title" "placeholder" "label" "aria-label" "aria-description"))
