/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use cssparser::Parser;
use media_queries::CSSErrorReporterTest;
use servo_url::ServoUrl;
use style::parser::ParserContext;
use style::properties::longhands::{mask_clip, mask_composite, mask_image, mask_mode};
use style::properties::longhands::{mask_origin, mask_position_x, mask_position_y, mask_repeat, mask_size};
use style::properties::shorthands::mask;
use style::stylesheets::{CssRuleType, Origin};

#[test]
fn mask_shorthand_should_parse_all_available_properties_when_specified() {
    let url = ServoUrl::parse("http://localhost").unwrap();
    let reporter = CSSErrorReporterTest;
    let context = ParserContext::new(Origin::Author, &url, &reporter, Some(CssRuleType::Style));
    let mut parser = Parser::new("url(\"http://servo/test.png\") luminance 7px 4px / 70px 50px \
                                 repeat-x padding-box border-box subtract");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_image, parse_longhand!(mask_image, "url(\"http://servo/test.png\")"));
    assert_eq!(result.mask_mode, parse_longhand!(mask_mode, "luminance"));
    assert_eq!(result.mask_position_x, parse_longhand!(mask_position_x, "7px"));
    assert_eq!(result.mask_position_y, parse_longhand!(mask_position_y, "4px"));
    assert_eq!(result.mask_size, parse_longhand!(mask_size, "70px 50px"));
    assert_eq!(result.mask_repeat, parse_longhand!(mask_repeat, "repeat-x"));
    assert_eq!(result.mask_origin, parse_longhand!(mask_origin, "padding-box"));
    assert_eq!(result.mask_clip, parse_longhand!(mask_clip, "border-box"));
    assert_eq!(result.mask_composite, parse_longhand!(mask_composite, "subtract"));
}

#[test]
fn mask_shorthand_should_parse_when_some_fields_set() {
    let url = ServoUrl::parse("http://localhost").unwrap();
    let reporter = CSSErrorReporterTest;
    let context = ParserContext::new(Origin::Author, &url, &reporter, Some(CssRuleType::Style));
    let mut parser = Parser::new("14px 40px repeat-y");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_position_x, parse_longhand!(mask_position_x, "14px"));
    assert_eq!(result.mask_position_y, parse_longhand!(mask_position_y, "40px"));
    assert_eq!(result.mask_repeat, parse_longhand!(mask_repeat, "repeat-y"));

    let mut parser = Parser::new("url(\"http://servo/test.png\") repeat add");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_image, parse_longhand!(mask_image, "url(\"http://servo/test.png\")"));
    assert_eq!(result.mask_repeat, parse_longhand!(mask_repeat, "repeat"));
    assert_eq!(result.mask_composite, parse_longhand!(mask_composite, "add"));

    let mut parser = Parser::new("intersect");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_composite, parse_longhand!(mask_composite, "intersect"));

    let mut parser = Parser::new("url(\"http://servo/test.png\")");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_image, parse_longhand!(mask_image, "url(\"http://servo/test.png\")"));
}

#[test]
fn mask_shorthand_should_parse_position_and_size_correctly() {
    let url = ServoUrl::parse("http://localhost").unwrap();
    let reporter = CSSErrorReporterTest;
    let context = ParserContext::new(Origin::Author, &url, &reporter, Some(CssRuleType::Style));
    let mut parser = Parser::new("7px 4px");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_position_x, parse_longhand!(mask_position_x, "7px"));
    assert_eq!(result.mask_position_y, parse_longhand!(mask_position_y, "4px"));

    let mut parser = Parser::new("7px 4px / 30px 20px");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_position_x, parse_longhand!(mask_position_x, "7px"));
    assert_eq!(result.mask_position_y, parse_longhand!(mask_position_y, "4px"));
    assert_eq!(result.mask_size, parse_longhand!(mask_size, "30px 20px"));

    let mut parser = Parser::new("/ 30px 20px");
    assert!(mask::parse_value(&context, &mut parser).is_err());

    let mut parser = Parser::new("match-source repeat-x / 30px 20px");
    assert!(mask::parse_value(&context, &mut parser).is_err());
}

#[test]
fn mask_shorthand_should_parse_origin_and_clip_correctly() {
    let url = ServoUrl::parse("http://localhost").unwrap();
    let reporter = CSSErrorReporterTest;
    let context = ParserContext::new(Origin::Author, &url, &reporter, Some(CssRuleType::Style));
    let mut parser = Parser::new("padding-box content-box");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_origin, parse_longhand!(mask_origin, "padding-box"));
    assert_eq!(result.mask_clip, parse_longhand!(mask_clip, "content-box"));

    let mut parser = Parser::new("padding-box padding-box");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_origin, parse_longhand!(mask_origin, "padding-box"));
    assert_eq!(result.mask_clip, parse_longhand!(mask_clip, "padding-box"));

    let mut parser = Parser::new("padding-box");
    let result = mask::parse_value(&context, &mut parser).unwrap();

    assert_eq!(result.mask_origin, parse_longhand!(mask_origin, "padding-box"));
    assert_eq!(result.mask_clip, parse_longhand!(mask_clip, "padding-box"));
}

#[test]
fn mask_shorthand_should_parse_mode_everywhere() {
    let url = ServoUrl::parse("http://localhost").unwrap();
    let reporter = CSSErrorReporterTest;
    let context = ParserContext::new(Origin::Author, &url, &reporter, Some(CssRuleType::Style));
    let mut parser = Parser::new("luminance 7px 4px repeat-x padding-box");
    assert!(mask::parse_value(&context, &mut parser).is_ok());

    let mut parser = Parser::new("alpha");
    assert!(mask::parse_value(&context, &mut parser).is_ok());
}

#[test]
fn mask_repeat_should_parse_shorthand_correctly() {
    use style::properties::longhands::mask_repeat::single_value::{RepeatKeyword, SpecifiedValue};

    let repeat_x = parse_longhand!(mask_repeat, "repeat-x");
    assert_eq!(repeat_x, mask_repeat::SpecifiedValue(vec![SpecifiedValue::RepeatX]));

    let repeat_y = parse_longhand!(mask_repeat, "repeat-y");
    assert_eq!(repeat_y, mask_repeat::SpecifiedValue(vec![SpecifiedValue::RepeatY]));

    let repeat = parse_longhand!(mask_repeat, "repeat");
    assert_eq!(repeat,
               mask_repeat::SpecifiedValue(vec![SpecifiedValue::Other(RepeatKeyword::Repeat, None)]));

    let space = parse_longhand!(mask_repeat, "space");
    assert_eq!(space,
               mask_repeat::SpecifiedValue(vec![SpecifiedValue::Other(RepeatKeyword::Space, None)]));

    let round = parse_longhand!(mask_repeat, "round");
    assert_eq!(round,
               mask_repeat::SpecifiedValue(vec![SpecifiedValue::Other(RepeatKeyword::Round, None)]));

    let no_repeat = parse_longhand!(mask_repeat, "no-repeat");
    assert_eq!(no_repeat,
               mask_repeat::SpecifiedValue(vec![SpecifiedValue::Other(RepeatKeyword::NoRepeat, None)]));
}

#[test]
fn mask_repeat_should_parse_longhand_correctly() {
    use style::properties::longhands::mask_repeat::single_value::{RepeatKeyword, SpecifiedValue};

    let url = ServoUrl::parse("http://localhost").unwrap();
    let reporter = CSSErrorReporterTest;
    let context = ParserContext::new(Origin::Author, &url, &reporter, Some(CssRuleType::Style));

    // repeat-x is not available in longhand form.
    let mut parser = Parser::new("repeat-x no-repeat");
    assert!(mask_repeat::parse(&context, &mut parser).is_err());

    let mut parser = Parser::new("no-repeat repeat-x");
    assert!(mask_repeat::parse(&context, &mut parser).is_err());

    // repeat-y is not available in longhand form.
    let mut parser = Parser::new("repeat-y no-repeat");
    assert!(mask_repeat::parse(&context, &mut parser).is_err());

    let mut parser = Parser::new("no-repeat repeat-y");
    assert!(mask_repeat::parse(&context, &mut parser).is_err());

    // Longhand form supports two directions.
    let no_repeat_and_round = parse_longhand!(mask_repeat, "no-repeat round");
    assert_eq!(no_repeat_and_round,
               mask_repeat::SpecifiedValue(vec![SpecifiedValue::Other(RepeatKeyword::NoRepeat,
                                                                      Some(RepeatKeyword::Round))]));

    // Not three directions.
    let mut parser = Parser::new("repeat no-repeat round");
    assert!(mask_repeat::parse(&context, &mut parser).is_err());

    // Multiple values with mixed shortform and longform should parse.
    let multiple = parse_longhand!(mask_repeat, "repeat, no-repeat round");
    assert_eq!(multiple,
               mask_repeat::SpecifiedValue(vec![SpecifiedValue::Other(RepeatKeyword::Repeat, None),
                                                SpecifiedValue::Other(RepeatKeyword::NoRepeat,
                                                                      Some(RepeatKeyword::Round))]));
}
