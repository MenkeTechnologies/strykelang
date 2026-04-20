//! Extra tests for Object-Oriented features in Stryke.

use crate::run;

#[test]
fn test_bless_and_ref() {
    let code = r#"
        my $obj = bless { a => 1 }, "My::Class";
        ref($obj);
    "#;
    assert_eq!(run(code).expect("run").to_string(), "My::Class");
}

#[test]
fn test_method_call_basic() {
    let code = r#"
        package Foo {
            sub new { bless { val => $_[1] }, "Foo" }
            sub get_val { $_[0]->{val} }
        }
        my $obj = Foo->new(42);
        $obj->get_val();
    "#;
    assert_eq!(run(code).expect("run").to_int(), 42);
}

#[test]
fn test_inheritance_isa() {
    let code = r#"
        package Parent {
            sub identify { "parent" }
        }
        package Child {
            our @ISA = ("Parent");
        }
        my $obj = bless {}, "Child";
        $obj->identify();
    "#;
    assert_eq!(run(code).expect("run").to_string(), "parent");
}

#[test]
fn test_can_method() {
    let code = r#"
        package TestCan {
            sub existing { 1 }
        }
        my $obj = bless {}, "TestCan";
        my $c1 = $obj->can("existing") ? "yes" : "no";
        my $c2 = $obj->can("nonexistent") ? "yes" : "no";
        "$c1:$c2";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "yes:no");
}

#[test]
fn test_nested_packages() {
    let code = r#"
        package Outer::Inner {
            sub hello { "inner" }
        }
        Outer::Inner->hello();
    "#;
    assert_eq!(run(code).expect("run").to_string(), "inner");
}

#[test]
fn test_isa_builtin() {
    let code = r#"
        package Base { }
        package Derived { our @ISA = ("Base") }
        my $obj = bless {}, "Derived";
        my $i1 = $obj->isa("Derived") ? 1 : 0;
        my $i2 = $obj->isa("Base") ? 1 : 0;
        my $i3 = $obj->isa("UNIVERSAL") ? 1 : 0;
        my $i4 = $obj->isa("Other") ? 1 : 0;
        "$i1$i2$i3$i4";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "1110");
}

#[test]
fn test_method_override() {
    // Renamed 'm' to 'meth' to avoid confusion with 'm' operator
    let code = r#"
        package P { sub meth { "P" } }
        package C { our @ISA = ("P"); sub meth { "C" } }
        my $obj = bless {}, "C";
        $obj->meth();
    "#;
    assert_eq!(run(code).expect("run").to_string(), "C");
}

#[test]
fn test_explicit_package_call() {
    let code = r#"
        package P { sub meth { "P" } }
        package C { our @ISA = ("P"); sub meth { P->meth() . "C" } }
        my $obj = bless {}, "C";
        $obj->meth();
    "#;
    assert_eq!(run(code).expect("run").to_string(), "PC");
}
