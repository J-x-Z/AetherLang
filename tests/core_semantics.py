#!/usr/bin/env python3
"""Run Core v1 semantic acceptance cases through the Aether compiler itself."""
import argparse
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
args = parser.parse_args()
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work)}
records = []
positive = {
    'main_with_arguments': 'fn main(count:i32,args:**u8)->i32{return count-1;}',
    'main_void': 'fn main(){return;}',
    'forward_calls': 'fn first(x:i32)->i32{return second(x);}fn second(x:i32)->i32{return x+1;}fn main()->i32{return first(1)-2;}',
    'recursive_pointer_structs': 'struct A{b:*B,value:i32}struct B{a:*A}fn f(a:*A)->i32{return a.b.a.value;}fn main()->i32{return 0;}',
    'lexical_shadow': 'fn f(x:i32)->i32{let mut y:i32=x;{let x:bool=true;if x{y=2;}}return y+x;}fn main()->i32{return f(1)-3;}',
    'parameter_and_pointer_mutability': 'struct S{x:i32}fn f(mut value:i32,p:*S)->i32{value=value+1;p.x=value;return p.x;}fn main()->i32{return 0;}',
    'return_branches': 'fn f(x:bool)->i32{if x{return 1;}else{return 2;}}fn main()->i32{return f(false)-2;}',
    'loop_jumps': 'fn f()->i32{let mut i:i32=0;while i<10{i=i+1;if i==3{continue;}if i==5{break;}}return i;}fn main()->i32{return f()-5;}',
    'literal_boundaries': 'const LOW:i8=-128;const HIGH:u8=255;const HEX:u32=0xffffffff;fn main()->i32{let small:i16=-32768;let high:u64=18446744073709551615;return 0;}',
    'scalar_and_pointer_casts': 'fn f(value:f64,p:*u8)->i32{let n:i32=value as i32;let q:*i32=p as *i32;let bits:u64=q as u64;return n+(bits as i32);}fn main()->i32{return 0;}',
    'address_and_index': 'fn main()->i32{let mut value:i32=1;let pointer:*i32=&mut value;pointer[0]=2;return value-2;}',
    'foreign_varargs': 'extern "C"{fn printf(format:*u8,...)->i32;}fn main()->i32{printf("%d %f",1,2.0);return 0;}',
    'sizeof_forward': 'struct A{next:*A,value:i32}fn main()->i32{let bytes:i64=size_of(A);return 0;}',
}
negative = {
    'main_bad_return': ('fn main()->f64{return 1.0;}', 'main requires'),
    'main_bad_parameters': ('fn main(x:i32)->i32{return x;}', 'main parameters'),
    'main_bad_argv': ('fn main(x:i32,args:*u8)->i32{return x;}', 'main parameters'),
    'main_extra_parameter': ('fn main(x:i32,args:**u8,y:i32)->i32{return x;}', 'main parameters'),
    'main_foreign': ('extern "C"{fn main()->i32;}', 'main requires'),
    'float_overflow': ('fn main()->i32{let x:f64=1e999;return 0;}', 'floating literal is outside'),
    'float32_overflow': ('fn main()->i32{let x:f32=1e39;return 0;}', 'floating literal is outside'),
    'constant_signed_overflow': ('const X:i8=-(-128);fn main()->i32{return 0;}', 'constant negation overflows'),
    'constant_unsigned_overflow': ('const X:u8=-(~0);fn main()->i32{return 0;}', 'constant negation overflows'),
    'constant_duplicate': ('const X:i32=1;const X:i32=2;fn main()->i32{return X;}', 'duplicate'),
    'constant_assignment': ('const X:i32=1;fn main()->i32{X=2;return 0;}', 'immutable'),
    'expression_assignment': ('fn main()->i32{(1+2)=3;return 0;}', 'assignable place'),
    'call_assignment': ('fn f()->i32{return 1;}fn main()->i32{f()=3;return 0;}', 'assignable place'),
    'float_pointer_cast': ('fn main()->i32{let p:*u8=1.5 as *u8;return 0;}', 'invalid scalar'),
    'unknown_name': ('fn main()->i32{return missing;}', 'unknown value'),
    'unknown_function': ('fn main()->i32{return missing();}', 'unknown function'),
    'wrong_argument_type': ('fn f(x:i32)->i32{return x;}fn main()->i32{return f(true);}', 'type mismatch'),
    'too_few_arguments': ('fn f(x:i32)->i32{return x;}fn main()->i32{return f();}', 'too few'),
    'too_many_arguments': ('fn f()->i32{return 0;}fn main()->i32{return f(1);}', 'too many'),
    'bad_return_type': ('fn main()->i32{return true;}', 'type mismatch'),
    'bare_value_return': ('fn main()->i32{return;}', 'return needs'),
    'void_value_return': ('fn f(){return 1;}fn main()->i32{return 0;}', 'void function'),
    'missing_return': ('fn main()->i32{let value:i32=1;}', 'without returning'),
    'partial_return': ('fn f(x:bool)->i32{if x{return 1;}}fn main()->i32{return 0;}', 'without returning'),
    'immutable_local': ('fn main()->i32{let x:i32=1;x=2;return x;}', 'immutable'),
    'immutable_parameter': ('fn f(x:i32)->i32{x=2;return x;}fn main()->i32{return 0;}', 'immutable'),
    'immutable_field': ('struct S{x:i32}fn f(s:S){s.x=1;}fn main()->i32{return 0;}', 'immutable'),
    'immutable_mut_address': ('fn main()->i32{let x:i32=1;let p:*i32=&mut x;return 0;}', 'immutable'),
    'scope_escape': ('fn main()->i32{{let x:i32=1;}return x;}', 'unknown value'),
    'duplicate_scope': ('fn main()->i32{let x:i32=1;let x:i32=2;return x;}', 'duplicate binding'),
    'duplicate_parameter': ('fn f(x:i32,x:i32){}fn main()->i32{return 0;}', 'duplicate function parameter'),
    'duplicate_function': ('fn f(){}fn f(){}fn main()->i32{return 0;}', 'duplicate'),
    'duplicate_field': ('struct S{x:i32,x:i32}fn main()->i32{return 0;}', 'duplicate struct field'),
    'unknown_type': ('fn f(p:*Missing){}fn main()->i32{return 0;}', 'unknown struct'),
    'infinite_struct': ('struct S{value:S}fn main()->i32{return 0;}', 'recursive struct'),
    'mutual_infinite_struct': ('struct A{value:B}struct B{value:A}fn main()->i32{return 0;}', 'recursive struct'),
    'unknown_field': ('struct S{x:i32}fn f(s:*S)->i32{return s.y;}fn main()->i32{return 0;}', 'unknown struct field'),
    'bool_condition': ('fn main()->i32{if 1{return 1;}return 0;}', 'condition must be bool'),
    'bool_logical': ('fn main()->i32{let b:bool=1&&2;return 0;}', 'logical operands'),
    'integer_overflow': ('fn main()->i32{let x:u8=256;return 0;}', 'outside its type range'),
    'negative_overflow': ('fn main()->i32{let x:i8=-129;return 0;}', 'outside its type range'),
    'unsigned_negative': ('fn main()->i32{let x:u8=-1;return 0;}', 'signed integer'),
    'integer_mixed_width': ('fn main()->i32{let x:i64=1;let y:i32=2;return x+y;}', 'operand types differ'),
    'void_deref': ('fn f(p:*void)->i32{return *p;}fn main()->i32{return 0;}', 'non-void pointer'),
    'bad_index': ('fn f(p:*i32)->i32{return p[true];}fn main()->i32{return 0;}', 'integer index'),
    'bad_cast': ('fn f(p:*i32)->f64{return p as f64;}fn main()->i32{return 0;}', 'invalid scalar'),
    'non_place': ('fn main()->i32{let p:*i64=&(1+2);return 0;}', 'address requires'),
    'constant_address': ('const X:i32=1;fn main()->i32{let p:*i32=&X;return 0;}', 'constant has no'),
    'constant_call': ('fn f()->i32{return 1;}const X:i32=f();fn main()->i32{return 0;}', 'Core constants'),
    'outside_break': ('fn main()->i32{break;return 0;}', 'outside a loop'),
    'outside_continue': ('fn main()->i32{continue;return 0;}', 'outside a loop'),
    'function_shadow': ('fn f()->i32{return 0;}fn main()->i32{let f:i32=1;return f();}', 'shadows function'),
}
# Malformed source must fail normally, including bounded recursion diagnostics.
negative.update({'parser_no_initializer': ('fn f(){let x:i32;}', 'Core let requires an initializer'),
 'parser_missing_type': ('fn f(){let x=1;}', "let requires an explicit ': Type'"),
 'parser_wrong_foreign_abi': ('extern "Rust"{fn f();}', 'Core supports only extern "C"'),
 'parser_embedded_abi': ('extern "C\\0evil"{fn f();}', 'Core supports only extern "C"'),
 'parser_foreign_body': ('extern "C"{fn f(){}}', "foreign function requires ';', not a body"),
 'parser_variadic_without_fixed': ('extern "C"{fn f(...);}',
                                   'variadic function needs a fixed parameter'),
 'parser_variadic_not_last': ('extern "C"{fn f(x:i32,...,y:i32);}', 'variadic marker must be last'),
 'parser_unclosed': ('fn f(){return;', 'unterminated block'),
 'parser_missing_semicolon': ('fn f(){let x:i32=0 return;}', "expected ';' after let"),
 'parser_unsupported': ('enum E{A}',
                        'expected fn, struct, const or extern "C"; construct outside Core v1'),
 'parser_assign_expr': ('fn f()->i32{return 1=2;}', "expected ';' after return"),
 'parser_indirect_call': ('fn f()->i32{return (*pointer)(1);}',
                          'Core calls require a function name'),
 'parser_deep_expr': ('fn f()->i32{return '
                      '((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))));}',
                      'expression nesting exceeds 256'),
 'parser_deep_type': ('fn '
                      'f(x:************************************************************************************************************************************************************************************************************************************************************************************************************u8){}',
                      'type nesting exceeds 256'),
 'parser_deep_if': ('fn f(){if true{}else if true{}else if true{}else if true{}else if true{}else '
                    'if true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else if '
                    'true{}else if true{}else if true{}else if true{}else if true{}else {}}',
                    'expression nesting exceeds 256')})
negative.update({
    'parser_deep_left_tree': ('fn main()->i32{return ' + '+'.join(['1'] * 300) + ';}',
                              'expression tree depth exceeds 256'),
    'parser_deep_postfix_tree': ('struct S{next:*S}fn f(p:*S)->*S{return p' + '.next' * 300 + ';}',
                                 'expression tree depth exceeds 256'),
    'parser_deep_blocks': ('fn f(){' + '{' * 300 + 'return;' + '}' * 300 + '}',
                           'block nesting exceeds 256'),
})
for success, cases in [(True, positive), (False, negative)]:
    for name, value in cases.items():
        source, message = (value, '') if success else value
        path = work / f'{name}.aeth'
        path.write_text(source + '\n')
        command = [str(compiler), str(path), '--check']
        result = subprocess.run(command, env=env, text=True, capture_output=True, timeout=10)
        records.append(dict(name=name, command=command, expected_success=success,
                            status=result.returncode, stdout=result.stdout, stderr=result.stderr))
        (work / 'results.json').write_text(json.dumps(records, indent=2))
        assert result.returncode == (0 if success else 1), f'{name}: {result.returncode}\n{result.stdout}\n{result.stderr}'
        if not success:
            diagnostic = result.stdout + result.stderr
            assert message in diagnostic, f'{name}: expected {message!r}: {diagnostic}'
            assert str(path) in diagnostic, f'{name}: missing source location: {diagnostic}'
print(f'{len(records)} Core semantic cases passed')
