[
  "FROM"
  "RUN"
  "CMD"
  "LABEL"
  "MAINTAINER"
  "EXPOSE"
  "ENV"
  "ADD"
  "COPY"
  "ENTRYPOINT"
  "VOLUME"
  "USER"
  "WORKDIR"
  "ARG"
  "ONBUILD"
  "STOPSIGNAL"
  "HEALTHCHECK"
  "SHELL"
] @keyword

(comment) @comment

[
  (double_quoted_string)
  (single_quoted_string)
  (unquoted_string)
] @string

(escape_sequence) @string
(variable) @property
