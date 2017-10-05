//! UnescapedText is a tokenizer impl
//! for tokenizing character literals,
//! and escaped character literals (of allowed escapes),
//! into an unescaped text byte array

use std::iter::Peekable;
use std::slice::Iter;
use std::str::Chars;
use std::char::from_u32;
use std::process::exit;
use cli;
use itertools::PutBackN;
use super::token;

pub struct UnescapedText(Vec<u8>);
impl UnescapedText {
    fn new() -> UnescapedText {
        UnescapedText(Vec::new())
    }
    // take an iterator to the format string
    // consume between min and max chars
    // and return it as a base-X number
    fn base_to_u32(min_chars: u8, max_chars: u8, base: u32, it: &mut PutBackN<Chars>) -> u32 {
        let mut retval: u32 = 0;
        let mut found = 0;
        while found < max_chars {
            // if end of input break
            let nc = it.next();
            match nc {
                Some(digit) => {
                    // if end of hexchars break
                    match digit.to_digit(base) {
                        Some(d) => {
                            found += 1;
                            retval *= base;
                            retval += d;
                        }
                        None => {
                            it.put_back(digit);
                            break;
                        }
                    }
                }
                None => {
                    break;
                }
            }
        }
        if found < min_chars {
            // only ever expected for hex
            println!("missing hexadecimal number in escape"); //todo stderr
            exit(cli::EXIT_ERR);
        }
        retval
    }
    // validates against valid
    // IEC 10646 vals - these values
    // are pinned against the more popular
    // printf so as to not disrupt when
    // dropped-in as a replacement.
    fn validate_iec(val: u32, eight_word: bool) {
        let mut preface = 'u';
        let mut leading_zeros = 4;
        if eight_word {
            preface = 'U';
            leading_zeros = 8;
        }
        let err_msg = format!("invalid universal character name {0}{1:02$x}",
                              preface,
                              val,
                              leading_zeros);
        if (val < 159 && (val != 36 && val != 64 && val != 96)) || (val > 55296 && val < 57343) {
            println!("{}", err_msg);//todo stderr
            exit(cli::EXIT_ERR);
        }
    }
    // pass an iterator that succeeds an '/',
    // and process the remaining character
    // adding the unescaped bytes
    // to the passed byte_vec
    // in subs_mode change octal behavior
    fn handle_escaped(byte_vec: &mut Vec<u8>, it: &mut PutBackN<Chars>, subs_mode: bool) {
        let ch = match it.next() {
            Some(x) => x,
            None => '\\',
        };
        match ch {
            '0'...'9' | 'x' => {
                let min_len = 1;
                let mut max_len = 2;
                let mut base = 16;
                let ignore = false;
                match ch {
                    'x' => {}
                    e @ '0'...'9' => {
                        max_len = 3;
                        base = 8;
                        // in practice, gnu coreutils printf
                        // interprets octals without a
                        // leading zero in %b
                        // but it only skips leading zeros
                        // in %b mode.
                        // if we ever want to match gnu coreutil
                        // printf's docs instead of its behavior
                        // we'd set this to true.
                        // if subs_mode && e != '0'
                        //  { ignore = true; }
                        if !subs_mode || e != '0' {
                            it.put_back(ch);
                        }
                    }
                    _ => {}
                }
                if !ignore {
                    let val = (UnescapedText::base_to_u32(min_len, max_len, base, it) % 256) as u8;
                    byte_vec.push(val);
                    let bvec = [val];
                    cli::flush_bytes(&bvec);
                } else {
                    byte_vec.push(ch as u8);
                }
            }
            e @ _ => {
                // only for hex and octal
                // is byte encoding specified.
                // otherwise, why not leave the door open
                // for other encodings unless it turns out
                // a bottleneck.
                let mut s = String::new();
                let ch = match e {
                    '\\' => '\\',
                    '"' => '"',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    // bell
                    'a' => '\x07',
                    // backspace
                    'b' => '\x08',
                    // vertical tab
                    'v' => '\x0B',
                    // form feed
                    'f' => '\x0C',
                    // escape character
                    'e' => '\x1B',
                    'c' => exit(cli::EXIT_OK),
                    'u' | 'U' => {
                        let len = match e {
                            'u' => 4,
                            'U' | _ => 8,
                        };
                        let val = UnescapedText::base_to_u32(len, len, 16, it);
                        UnescapedText::validate_iec(val, false);
                        if let Some(c) = from_u32(val) {
                            c
                        } else {
                            '-'
                        }
                    }
                    _ => {
                        s.push('\\');
                        ch
                    }
                };
                s.push(ch);
                cli::flush_str(&s);
                byte_vec.extend(s.bytes());
            }
        };

    }

    // take an iterator to a string,
    // and return a wrapper around a Vec<u8> of unescaped bytes
    // break on encounter of sub symbol ('%[^%]') unless called
    // through %b subst.
    pub fn from_it_core(it: &mut PutBackN<Chars>, subs_mode: bool) -> Option<Box<token::Token>> {
        let mut addchar = false;
        let mut new_text = UnescapedText::new();
        let mut tmp_str = String::new();
        {
            let new_vec: &mut Vec<u8> = &mut (new_text.0);
            while let Some(ch) = it.next() {
                if !addchar {
                    addchar = true;
                }
                match ch as char {
                    x if x != '\\' && x != '%' => {
                        // lazy branch eval
                        // remember this fn could be called
                        // many times in a single exec through %b
                        cli::flush_char(&ch);
                        tmp_str.push(ch);
                    }
                    '\\' => {
                        // the literal may be a literal bytecode
                        // and not valid utf-8. Str only supports
                        // valid utf-8.
                        // if we find the unnecessary drain
                        // on non hex or octal escapes is costly
                        // then we can make it faster/more complex
                        // with as-necessary draining.
                        if tmp_str.len() > 0 {
                            new_vec.extend(tmp_str.bytes());
                            tmp_str = String::new();
                        }
                        UnescapedText::handle_escaped(new_vec, it, subs_mode)
                    }
                    x if x == '%' && !subs_mode => {
                        if let Some(follow) = it.next() {
                            if follow == '%' {
                                cli::flush_char(&ch);
                                tmp_str.push(ch);
                            } else {
                                it.put_back(follow);
                                it.put_back(ch);
                                break;
                            }
                        } else {
                            it.put_back(ch);
                            break;
                        }
                    }
                    _ => {
                        cli::flush_char(&ch);
                        tmp_str.push(ch);
                    }
                }
            }
            if tmp_str.len() > 0 {
                new_vec.extend(tmp_str.bytes());
            }
        }
        match addchar {
            true => Some(Box::new(new_text)),
            false => None,
        }
    }
}
#[allow(unused_variables)]
impl token::Tokenizer for UnescapedText {
    fn from_it(it: &mut PutBackN<Chars>,
               args: &mut Peekable<Iter<String>>)
               -> Option<Box<token::Token>> {
        UnescapedText::from_it_core(it, false)
    }
}
#[allow(unused_variables)]
impl token::Token for UnescapedText {
    fn print(&self, pf_args_it: &mut Peekable<Iter<String>>) {
        cli::flush_bytes(&self.0[..]);
    }
}
