//! High-level types and functions related to CSS parsing
use std::{
    num::ParseIntError,
};
pub use simplecss::Error as CssSyntaxError;

use css_parser;
pub use css_parser::CssParsingError;
use dom::{node_type_path_from_str, NodeTypePathParseError};
use azul_css::{
    Css,
    CssDeclaration,
    DynamicCssProperty,
    DynamicCssPropertyDefault,
    CssRuleBlock,
    CssPath,
    CssPathSelector,
    CssPathPseudoSelector,
};

/// Error that can happen during the parsing of a CSS value
#[derive(Debug, Clone, PartialEq)]
pub enum CssParseError<'a> {
    /// A hard error in the CSS syntax
    ParseError(CssSyntaxError),
    /// Braces are not balanced properly
    UnclosedBlock,
    /// Invalid syntax, such as `#div { #div: "my-value" }`
    MalformedCss,
    /// Error parsing dynamic CSS property, such as
    /// `#div { width: {{ my_id }} /* no default case */ }`
    DynamicCssParseError(DynamicCssParseError<'a>),
    /// Error during parsing the value of a field
    /// (Css is parsed eagerly, directly converted to strongly typed values
    /// as soon as possible)
    UnexpectedValue(CssParsingError<'a>),
    /// Error while parsing a pseudo selector (like `:aldkfja`)
    PseudoSelectorParseError(CssPseudoSelectorParseError<'a>),
    /// The path has to be either `*`, `div`, `p` or something like that
    NodeTypePath(NodeTypePathParseError<'a>),
}

impl_display!{ CssParseError<'a>, {
    ParseError(e) => format!("Parse Error: {:?}", e),
    UnclosedBlock => "Unclosed block",
    MalformedCss => "Malformed Css",
    DynamicCssParseError(e) => format!("Dynamic parsing error: {}", e),
    UnexpectedValue(e) => format!("Unexpected value: {}", e),
    PseudoSelectorParseError(e) => format!("Failed to parse pseudo-selector: {}", e),
    NodeTypePath(e) => format!("Failed to parse CSS selector path: {}", e),
}}

impl_from! { CssParsingError<'a>, CssParseError::UnexpectedValue }
impl_from! { DynamicCssParseError<'a>, CssParseError::DynamicCssParseError }
impl_from! { CssPseudoSelectorParseError<'a>, CssParseError::PseudoSelectorParseError }
impl_from! { NodeTypePathParseError<'a>, CssParseError::NodeTypePath }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssPseudoSelectorParseError<'a> {
    UnknownSelector(&'a str),
    InvalidNthChild(ParseIntError),
    UnclosedBracesNthChild(&'a str),
}

impl<'a> From<ParseIntError> for CssPseudoSelectorParseError<'a> {
    fn from(e: ParseIntError) -> Self { CssPseudoSelectorParseError::InvalidNthChild(e) }
}

impl_display! { CssPseudoSelectorParseError<'a>, {
    UnknownSelector(e) => format!("Invalid CSS pseudo-selector: ':{}'", e),
    InvalidNthChild(e) => format!("Invalid :nth-child pseudo-selector: ':{}'", e),
    UnclosedBracesNthChild(e) => format!(":nth-child has unclosed braces: ':{}'", e),
}}

fn pseudo_selector_from_str<'a>(data: &'a str) -> Result<CssPathPseudoSelector, CssPseudoSelectorParseError<'a>> {
    match data {
        "first" => Ok(CssPathPseudoSelector::First),
        "last" => Ok(CssPathPseudoSelector::Last),
        "hover" => Ok(CssPathPseudoSelector::Hover),
        "active" => Ok(CssPathPseudoSelector::Active),
        "focus" => Ok(CssPathPseudoSelector::Focus),
        other => {
            // TODO: move this into a seperate function
            if other.starts_with("nth-child") {
                let mut nth_child = other.split("nth-child");
                nth_child.next();
                let mut nth_child_string = nth_child.next().ok_or(CssPseudoSelectorParseError::UnknownSelector(other))?;
                nth_child_string.trim();
                if !nth_child_string.starts_with("(") || !nth_child_string.ends_with(")") {
                    return Err(CssPseudoSelectorParseError::UnclosedBracesNthChild(other));
                }

                // Should the string be empty, then the `starts_with` and `ends_with` won't succeed
                let mut nth_child_string = &nth_child_string[1..nth_child_string.len() - 1];
                nth_child_string.trim();
                let parsed = nth_child_string.parse::<usize>()?;
                Ok(CssPathPseudoSelector::NthChild(parsed))
            } else {
                Err(CssPseudoSelectorParseError::UnknownSelector(other))
            }
        },
    }
}

#[test]
fn test_css_pseudo_selector_parse() {
    let ok_res = [
        ("first", CssPathPseudoSelector::First),
        ("last", CssPathPseudoSelector::Last),
        ("nth-child(4)", CssPathPseudoSelector::NthChild(4)),
        ("hover", CssPathPseudoSelector::Hover),
        ("active", CssPathPseudoSelector::Active),
        ("focus", CssPathPseudoSelector::Focus),
    ];

    let err = [
        ("asdf", CssPseudoSelectorParseError::UnknownSelector("asdf")),
        ("", CssPseudoSelectorParseError::UnknownSelector("")),
        ("nth-child(", CssPseudoSelectorParseError::UnclosedBracesNthChild("nth-child(")),
        ("nth-child)", CssPseudoSelectorParseError::UnclosedBracesNthChild("nth-child)")),
        // Can't test for ParseIntError because the fields are private.
        // This is an example on why you shouldn't use std::error::Error!
    ];

    for (s, a) in &ok_res {
        assert_eq!(pseudo_selector_from_str(s), Ok(*a));
    }

    for (s, e) in &err {
        assert_eq!(pseudo_selector_from_str(s), Err(e.clone()));
    }
}

/// Parses a CSS string (single-threaded) and returns the parsed rules in blocks
pub fn new_from_str<'a>(css_string: &'a str) -> Result<Css, CssParseError<'a>> {
    use simplecss::{Tokenizer, Token, Combinator};

    let mut tokenizer = Tokenizer::new(css_string);

    let mut css_blocks = Vec::new();

    // Used for error checking / checking for closed braces
    let mut parser_in_block = false;
    let mut block_nesting = 0_usize;

    // Current css paths (i.e. `div#id, .class, p` are stored here -
    // when the block is finished, all `current_rules` gets duplicated with
    // one path corresponding to one set of rules each).
    let mut current_paths = Vec::new();
    // Current CSS declarations
    let mut current_rules = Vec::new();
    // Keep track of the current path during parsing
    let mut last_path = Vec::new();

    loop {
        let tokenize_result = tokenizer.parse_next();
        match tokenize_result {
            Ok(token) => {
                match token {
                    Token::BlockStart => {
                        if parser_in_block {
                            // multi-nested CSS blocks are currently not supported
                            return Err(CssParseError::MalformedCss);
                        }
                        parser_in_block = true;
                        block_nesting += 1;
                        current_paths.push(last_path.clone());
                        last_path.clear();
                    },
                    Token::Comma => {
                        current_paths.push(last_path.clone());
                        last_path.clear();
                    },
                    Token::BlockEnd => {
                        block_nesting -= 1;
                        if !parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        parser_in_block = false;
                        for path in current_paths.drain(..) {
                            css_blocks.push(CssRuleBlock {
                                path: CssPath { selectors: path },
                                declarations: current_rules.clone(),
                            })
                        }
                        current_rules.clear();
                        last_path.clear(); // technically unnecessary, but just to be sure
                    },

                    // tokens that adjust the last_path
                    Token::UniversalSelector => {
                        if parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        last_path.push(CssPathSelector::Global);
                    },
                    Token::TypeSelector(div_type) => {
                        if parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        last_path.push(CssPathSelector::Type(node_type_path_from_str(div_type)?));
                    },
                    Token::IdSelector(id) => {
                        if parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        last_path.push(CssPathSelector::Id(id.to_string()));
                    },
                    Token::ClassSelector(class) => {
                        if parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        last_path.push(CssPathSelector::Class(class.to_string()));
                    },
                    Token::Combinator(Combinator::GreaterThan) => {
                        if parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        last_path.push(CssPathSelector::DirectChildren);
                    },
                    Token::Combinator(Combinator::Space) => {
                        if parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        last_path.push(CssPathSelector::Children);
                    },
                    Token::PseudoClass(pseudo_class) => {
                        if parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        last_path.push(CssPathSelector::PseudoSelector(pseudo_selector_from_str(pseudo_class)?));
                    },
                    Token::Declaration(key, val) => {
                        if !parser_in_block {
                            return Err(CssParseError::MalformedCss);
                        }
                        current_rules.push(determine_static_or_dynamic_css_property(key, val)?);
                    },
                    Token::EndOfStream => {
                        break;
                    },
                    _ => {
                        // attributes, lang-attributes and @keyframes are not supported
                    }
                }
            },
            Err(e) => {
                return Err(CssParseError::ParseError(e));
            }
        }
    }

    // non-even number of blocks
    if block_nesting != 0 {
        return Err(CssParseError::UnclosedBlock);
    }

    Ok(css_blocks.into())
}

/// Error that can happen during `css_parser::from_kv`
#[derive(Debug, Clone, PartialEq)]
pub enum DynamicCssParseError<'a> {
    /// The braces of a dynamic CSS property aren't closed or unbalanced, i.e. ` [[ `
    UnclosedBraces,
    /// There is a valid dynamic css property, but no default case
    NoDefaultCase,
    /// The dynamic CSS property has no ID, i.e. `[[ 400px ]]`
    NoId,
    /// The ID may not start with a number or be a CSS property itself
    InvalidId,
    /// Dynamic css property braces are empty, i.e. `[[ ]]`
    EmptyBraces,
    /// Unexpected value when parsing the string
    UnexpectedValue(CssParsingError<'a>),
}

impl_display!{ DynamicCssParseError<'a>, {
    UnclosedBraces => "The braces of a dynamic CSS property aren't closed or unbalanced, i.e. ` [[ `",
    NoDefaultCase => "There is a valid dynamic css property, but no default case",
    NoId => "The dynamic CSS property has no ID, i.e. [[ 400px ]]",
    InvalidId => "The ID may not start with a number or be a CSS property itself",
    EmptyBraces => "Dynamic css property braces are empty, i.e. `[[ ]]`",
    UnexpectedValue(e) => format!("Unexpected value: {}", e),
}}

impl<'a> From<CssParsingError<'a>> for DynamicCssParseError<'a> {
    fn from(e: CssParsingError<'a>) -> Self {
        DynamicCssParseError::UnexpectedValue(e)
    }
}

const START_BRACE: &str = "[[";
const END_BRACE: &str = "]]";

/// Determine if a Css property is static (immutable) or if it can change
/// during the runtime of the program
fn determine_static_or_dynamic_css_property<'a>(key: &'a str, value: &'a str)
-> Result<CssDeclaration, DynamicCssParseError<'a>>
{
    let key = key.trim();
    let value = value.trim();

    let is_starting_with_braces = value.starts_with(START_BRACE);
    let is_ending_with_braces = value.ends_with(END_BRACE);

    match (is_starting_with_braces, is_ending_with_braces) {
        (true, false) | (false, true) => {
            Err(DynamicCssParseError::UnclosedBraces)
        },
        (true, true) => {
            parse_dynamic_css_property(key, value).and_then(|val| Ok(CssDeclaration::Dynamic(val)))
        },
        (false, false) => {
            Ok(CssDeclaration::Static(css_parser::from_kv(key, value)?))
        }
    }
}

fn parse_dynamic_css_property<'a>(key: &'a str, value: &'a str) -> Result<DynamicCssProperty, DynamicCssParseError<'a>> {
    use std::char;

    // "[[ id | 400px ]]" => "id | 400px"
    let value = value.trim_left_matches(START_BRACE);
    let value = value.trim_right_matches(END_BRACE);
    let value = value.trim();

    let mut pipe_split = value.splitn(2, "|");
    let dynamic_id = pipe_split.next();
    let default_case = pipe_split.next();

    // note: dynamic_id will always be Some(), which is why the
    let (default_case, dynamic_id) = match (default_case, dynamic_id) {
        (Some(default), Some(id)) => (default, id),
        (None, Some(id)) => {
            if id.trim().is_empty() {
                return Err(DynamicCssParseError::EmptyBraces);
            } else if css_parser::from_kv(key, id).is_ok() {
                // if there is an ID, but the ID is a CSS value
                return Err(DynamicCssParseError::NoId);
            } else {
                return Err(DynamicCssParseError::NoDefaultCase);
            }
        },
        (None, None) | (Some(_), None) => unreachable!(), // iterator would be broken if this happened
    };

    let dynamic_id = dynamic_id.trim();
    let default_case = default_case.trim();

    match (dynamic_id.is_empty(), default_case.is_empty()) {
        (true, true) => return Err(DynamicCssParseError::EmptyBraces),
        (true, false) => return Err(DynamicCssParseError::NoId),
        (false, true) => return Err(DynamicCssParseError::NoDefaultCase),
        (false, false) => { /* everything OK */ }
    }

    if dynamic_id.starts_with(char::is_numeric) ||
       css_parser::from_kv(key, dynamic_id).is_ok() {
        return Err(DynamicCssParseError::InvalidId);
    }

    let default_case_parsed = match default_case {
        "auto" => DynamicCssPropertyDefault::Auto,
        other => DynamicCssPropertyDefault::Exact(css_parser::from_kv(key, other)?),
    };

    Ok(DynamicCssProperty {
        dynamic_id: dynamic_id.to_string(),
        default: default_case_parsed,
    })
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct CssConstraintList {
    pub(crate) list: Vec<CssDeclaration>
}

#[test]
fn test_detect_static_or_dynamic_property() {
    use azul_css::{CssProperty, StyleTextAlignmentHorz};
    use css_parser::InvalidValueErr;
    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", " center   "),
        Ok(CssDeclaration::Static(CssProperty::TextAlign(StyleTextAlignmentHorz::Center)))
    );

    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[    400px ]]"),
        Err(DynamicCssParseError::NoDefaultCase)
    );

    assert_eq!(determine_static_or_dynamic_css_property("text-align", "[[  400px"),
        Err(DynamicCssParseError::UnclosedBraces)
    );

    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[  400px | center ]]"),
        Err(DynamicCssParseError::InvalidId)
    );

    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[  hello | center ]]"),
        Ok(CssDeclaration::Dynamic(DynamicCssProperty {
            default: DynamicCssPropertyDefault::Exact(CssProperty::TextAlign(StyleTextAlignmentHorz::Center)),
            dynamic_id: String::from("hello"),
        }))
    );

    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[  hello | auto ]]"),
        Ok(CssDeclaration::Dynamic(DynamicCssProperty {
            default: DynamicCssPropertyDefault::Auto,
            dynamic_id: String::from("hello"),
        }))
    );

    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[  abc | hello ]]"),
        Err(DynamicCssParseError::UnexpectedValue(
            CssParsingError::InvalidValueErr(InvalidValueErr("hello"))
        ))
    );

    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[ ]]"),
        Err(DynamicCssParseError::EmptyBraces)
    );
    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[]]"),
        Err(DynamicCssParseError::EmptyBraces)
    );


    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[ center ]]"),
        Err(DynamicCssParseError::NoId)
    );

    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[ hello |  ]]"),
        Err(DynamicCssParseError::NoDefaultCase)
    );

    // debatable if this is a suitable error for this case:
    assert_eq!(
        determine_static_or_dynamic_css_property("text-align", "[[ |  ]]"),
        Err(DynamicCssParseError::EmptyBraces)
    );
}

#[test]
fn test_css_parse_1() {

    use azul_css::{ColorU, StyleBackgroundColor, NodeTypePath, CssProperty};

    let parsed_css = new_from_str("
        div#my_id .my_class:first {
            background-color: red;
        }
    ").unwrap();

    let expected_css_rules = vec![
        CssRuleBlock {
            path: CssPath {
                selectors: vec![
                    CssPathSelector::Type(NodeTypePath::Div),
                    CssPathSelector::Id(String::from("my_id")),
                    CssPathSelector::Children,
                    // NOTE: This is technically wrong, the space between "#my_id"
                    // and ".my_class" is important, but gets ignored for now
                    CssPathSelector::Class(String::from("my_class")),
                    CssPathSelector::PseudoSelector(CssPathPseudoSelector::First),
                ],
            },
            declarations: vec![CssDeclaration::Static(CssProperty::BackgroundColor(StyleBackgroundColor(ColorU { r: 255, g: 0, b: 0, a: 255 })))],
        }
    ];

    assert_eq!(parsed_css, expected_css_rules.into());
}

#[test]
fn test_css_simple_selector_parse() {
    use self::CssPathSelector::*;
    use azul_css::NodeTypePath;
    let css = "div#id.my_class > p .new { }";
    let parsed = vec![
        Type(NodeTypePath::Div),
        Id("id".into()),
        Class("my_class".into()),
        DirectChildren,
        Type(NodeTypePath::P),
        Children,
        Class("new".into())
    ];
    assert_eq!(new_from_str(css).unwrap(), Css {
        rules: vec![CssRuleBlock {
            path: CssPath { selectors: parsed },
            declarations: Vec::new(),
        }],
    });
}

#[cfg(test)]
mod stylesheet_parse {

    use azul_css::*;
    use super::*;

    fn test_css(css: &str, expected: Vec<CssRuleBlock>) {
        let css = new_from_str(css).unwrap();
        assert_eq!(css, expected.into());
    }

    // Tests that an element with a single class always gets the CSS element applied properly
    #[test]
    fn test_apply_css_pure_class() {
        let red = CssProperty::BackgroundColor(StyleBackgroundColor(ColorU { r: 255, g: 0, b: 0, a: 255 }));
        let blue = CssProperty::BackgroundColor(StyleBackgroundColor(ColorU { r: 0, g: 0, b: 255, a: 255 }));
        let black = CssProperty::BackgroundColor(StyleBackgroundColor(ColorU { r: 0, g: 0, b: 0, a: 255 }));

        // Simple example
        {
            let css_1 = ".my_class { background-color: red; }";
            let expected_rules = vec![
                CssRuleBlock {
                    path: CssPath { selectors: vec![CssPathSelector::Class("my_class".into())] },
                    declarations: vec![
                        CssDeclaration::Static(red.clone())
                    ],
                },
            ];
            test_css(css_1, expected_rules);
        }

        // Slightly more complex example
        {
            let css_2 = "#my_id { background-color: red; } .my_class { background-color: blue; }";
            let expected_rules = vec![
                CssRuleBlock {
                    path: CssPath { selectors: vec![CssPathSelector::Id("my_id".into())] },
                    declarations: vec![CssDeclaration::Static(red.clone())]
                },
                CssRuleBlock {
                    path: CssPath { selectors: vec![CssPathSelector::Class("my_class".into())] },
                    declarations: vec![CssDeclaration::Static(blue.clone())]
                },
            ];
            test_css(css_2, expected_rules);
        }

        // Even more complex example
        {
            let css_3 = "* { background-color: black; } .my_class#my_id { background-color: red; } .my_class { background-color: blue; }";
            let expected_rules = vec![
                CssRuleBlock {
                    path: CssPath { selectors: vec![CssPathSelector::Global] },
                    declarations: vec![CssDeclaration::Static(black.clone())]
                },
                CssRuleBlock {
                    path: CssPath { selectors: vec![CssPathSelector::Class("my_class".into()), CssPathSelector::Id("my_id".into())] },
                    declarations: vec![CssDeclaration::Static(red.clone())]
                },
                CssRuleBlock {
                    path: CssPath { selectors: vec![CssPathSelector::Class("my_class".into())] },
                    declarations: vec![CssDeclaration::Static(blue.clone())]
                },
            ];
            test_css(css_3, expected_rules);
        }
    }
}

// Assert that order of the style rules is correct (in same order as provided in CSS form)
#[test]
fn test_multiple_rules() {
    use azul_css::*;
    use self::CssPathSelector::*;

    let parsed_css = new_from_str("
        * { }
        * div.my_class#my_id { }
        * div#my_id { }
        * #my_id { }
        div.my_class.specific#my_id { }
    ").unwrap();

    let expected_rules = vec![
        // Rules are sorted by order of appearance in source string
        CssRuleBlock { path: CssPath { selectors: vec![Global] }, declarations: Vec::new() },
        CssRuleBlock { path: CssPath { selectors: vec![Global, Type(NodeTypePath::Div), Class("my_class".into()), Id("my_id".into())] }, declarations: Vec::new() },
        CssRuleBlock { path: CssPath { selectors: vec![Global, Type(NodeTypePath::Div), Id("my_id".into())] }, declarations: Vec::new() },
        CssRuleBlock { path: CssPath { selectors: vec![Global, Id("my_id".into())] }, declarations: Vec::new() },
        CssRuleBlock { path: CssPath { selectors: vec![Type(NodeTypePath::Div), Class("my_class".into()), Class("specific".into()), Id("my_id".into())] }, declarations: Vec::new() },
    ];

    assert_eq!(parsed_css, expected_rules.into());
}
