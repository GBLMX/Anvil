[
  (line_comment)
  (block_comment)
] @comment

[
  (string_literal)
  (char_literal)
  (raw_string_literal)
] @string

[
  (integer_literal)
  (float_literal)
] @number

(type_identifier) @type
(field_identifier) @property

(function_item name: (identifier) @function)
(call_expression function: (identifier) @function)
(macro_invocation macro: (identifier) @function)

