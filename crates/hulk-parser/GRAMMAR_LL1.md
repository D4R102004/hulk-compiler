# HULK parser: LL(1) grammar used by `hulk-parser`

This crate implements a hand-written predictive LL(1) parser. The code follows
the same transformation taught in the compiler course: remove ambiguity, remove
left recursion, factor common prefixes, and then choose each production with a
single token of lookahead.

The parser is not a generic table engine; each non-terminal is represented by a
Rust method. This is the usual practical implementation of a hand-written LL(1)
parser because semantic actions can construct the AST directly.

## Program level

```ebnf
Program      -> Declaration* Expr ';'* EOF
Declaration  -> FunctionDecl | TypeDecl | ProtocolDecl

FunctionDecl -> 'function' id FunctionTail
FunctionTail -> ParamList ReturnType? FunctionBody
ReturnType   -> ':' TypeRef
FunctionBody -> '=>' Expr ';'? | Block

TypeDecl     -> 'type' id ParamList? Parent? '{' TypeMember* '}'
Parent       -> 'inherits' id ConstructorArgs?
TypeMember   -> 'function' id FunctionTail
              | id TypeMemberAfterId
TypeMemberAfterId -> FunctionTail | TypeAnnotation? '=' Expr ';'

ProtocolDecl -> 'protocol' id ProtocolParents? '{' ProtocolMethod* '}'
```

Notice that `TypeMember` is left-factored: after seeing an identifier, the next
token chooses between a method (`(`) and an attribute (`:`, `=`).

## Expression levels

The natural expression grammar is left-recursive, so it is rewritten into a
sequence of LL(1) precedence levels. Loops in the implementation correspond to
`Tail -> op RHS Tail | epsilon` productions.

```ebnf
Expr        -> Assignment
Assignment  -> Or AssignmentTail
AssignmentTail -> ':=' Assignment | epsilon

Or          -> And OrTail
OrTail      -> '|' And OrTail | epsilon

And         -> Equality AndTail
AndTail     -> '&' Equality AndTail | epsilon

Equality    -> Comparison EqualityTail
EqualityTail -> ('==' | '!=') Comparison EqualityTail | epsilon

Comparison  -> TypeTest ComparisonTail
ComparisonTail -> ('<' | '<=' | '>' | '>=') TypeTest ComparisonTail | epsilon

TypeTest    -> Concat TypeTestTail
TypeTestTail -> ('is' | 'as') TypeRef TypeTestTail | epsilon

Concat      -> Term ConcatTail
ConcatTail  -> ('@' | '@@') Term ConcatTail | epsilon

Term        -> Factor TermTail
TermTail    -> ('+' | '-') Factor TermTail | epsilon

Factor      -> Unary FactorTail
FactorTail  -> ('*' | '/' | '%') Unary FactorTail | epsilon

Unary       -> '-' Unary | '!' Unary | Power
Power       -> Postfix PowerTail
PowerTail   -> '^' Unary | epsilon

Postfix     -> Primary PostfixTail
PostfixTail -> '(' ArgList? ')' PostfixTail
             | '.' id PostfixTail
             | '[' Expr ']' PostfixTail
             | epsilon
```

## Primary expressions

```ebnf
Primary -> number
         | string
         | 'true'
         | 'false'
         | id
         | 'self'
         | 'base'
         | '(' Expr ')'
         | Block
         | Vector
         | Let
         | If
         | While
         | For
         | New
         | Match
```

## Vector ambiguity resolution

HULK uses `|` both as Boolean OR and as the separator in vector comprehensions.
To keep the grammar LL(1), the parser uses a factored vector production and a
special head expression that does not consume top-level OR:

```ebnf
Vector      -> '[' VectorBody
VectorBody  -> ']'
            | ExprNoTopLevelOr VectorTail
VectorTail  -> '|' id 'in' Expr ']'
            | VectorItemsTail ']'
VectorItemsTail -> (',' Expr)*
```

This allows `[x^2 | x in range(1, 10)]` to be parsed as a comprehension, while
`[(x | y) | x in values]` remains valid because the OR expression is explicitly
parenthesized inside the head.

## Implementation map

| Grammar non-terminal | Rust method |
| --- | --- |
| `Program` | `parse_program` |
| `Declaration` | `parse_declaration` |
| `FunctionDecl` | `parse_function_declaration_after_keyword` |
| `TypeDecl` | `parse_type_declaration_after_keyword` |
| `ProtocolDecl` | `parse_protocol_declaration_after_keyword` |
| `Expr` | `parse_expression` |
| `Assignment` | `parse_assignment` |
| `Or` | `parse_or` |
| `And` | `parse_and` |
| `Equality` | `parse_equality` |
| `Comparison` | `parse_comparison` |
| `TypeTest` | `parse_type_test` |
| `Concat` | `parse_concat` |
| `Term` | `parse_term` |
| `Factor` | `parse_factor` |
| `Unary` | `parse_unary` |
| `Power` | `parse_power` |
| `Postfix` | `parse_postfix` |
| `Primary` | `parse_primary` |
