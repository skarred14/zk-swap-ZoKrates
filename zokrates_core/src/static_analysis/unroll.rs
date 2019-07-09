//! Module containing SSA reduction, including for-loop unrolling
//!
//! @file unroll.rs
//! @author Thibaut Schaeffer <thibaut@schaeff.fr>
//! @date 2018

use crate::typed_absy::folder::*;
use crate::typed_absy::*;
use crate::types::Type;
use std::collections::HashMap;
use zokrates_field::Field;

pub struct Unroller<'ast> {
    substitution: HashMap<Identifier<'ast>, usize>,
}

impl<'ast> Unroller<'ast> {
    fn new() -> Self {
        Unroller {
            substitution: HashMap::new(),
        }
    }

    fn issue_next_ssa_variable(&mut self, v: Variable<'ast>) -> Variable<'ast> {
        let res = match self.substitution.get(&v.id) {
            Some(i) => Variable {
                id: Identifier {
                    id: v.id.id,
                    version: i + 1,
                    stack: vec![],
                },
                ..v
            },
            None => Variable { ..v.clone() },
        };
        self.substitution
            .entry(v.id)
            .and_modify(|e| *e += 1)
            .or_insert(0);
        res
    }

    pub fn unroll<T: Field>(p: TypedProg<T>) -> TypedProg<T> {
        Unroller::new().fold_program(p)
    }
}

impl<'ast, T: Field> Folder<'ast, T> for Unroller<'ast> {
    fn fold_statement(&mut self, s: TypedStatement<'ast, T>) -> Vec<TypedStatement<'ast, T>> {
        match s {
            TypedStatement::Declaration(_) => vec![],
            TypedStatement::Definition(TypedAssignee::Identifier(variable), expr) => {
                let expr = self.fold_expression(expr);

                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(self.issue_next_ssa_variable(variable)),
                    expr,
                )]
            }
            TypedStatement::Definition(
                TypedAssignee::ArrayElement(array @ box TypedAssignee::Identifier(..), box index),
                expr,
            ) => {
                let expr = self.fold_expression(expr);
                let index = self.fold_field_expression(index);
                let current_array = self.fold_assignee(*array.clone());

                let current_ssa_variable = match current_array {
                    TypedAssignee::Identifier(v) => v,
                    _ => panic!("assignee should be an identifier"),
                };

                let original_variable = match *array {
                    TypedAssignee::Identifier(v) => v,
                    _ => panic!("assignee should be an identifier"),
                };

                let array_size = match original_variable.get_type() {
                    Type::FieldElementArray(size) => size,
                    _ => panic!("array identifier should be a field element array"),
                };

                let expr = match expr {
                    TypedExpression::FieldElement(e) => e,
                    _ => panic!("right side of array element definition must be a field element"),
                };

                let new_variable = self.issue_next_ssa_variable(original_variable);

                let new_array = FieldElementArrayExpression::Value(
                    array_size,
                    (0..array_size)
                        .map(|i| {
                            FieldElementExpression::IfElse(
                                box BooleanExpression::Eq(
                                    box index.clone(),
                                    box FieldElementExpression::Number(T::from(i)),
                                ),
                                box expr.clone(),
                                box FieldElementExpression::Select(
                                    box FieldElementArrayExpression::Identifier(
                                        array_size,
                                        current_ssa_variable.id.clone(),
                                    ),
                                    box FieldElementExpression::Number(T::from(i)),
                                ),
                            )
                        })
                        .collect(),
                );

                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(new_variable),
                    new_array.into(),
                )]
            }
            TypedStatement::MultipleDefinition(variables, exprs) => {
                let exprs = self.fold_expression_list(exprs);
                let variables = variables
                    .into_iter()
                    .map(|v| self.issue_next_ssa_variable(v))
                    .collect();

                vec![TypedStatement::MultipleDefinition(variables, exprs)]
            }
            TypedStatement::For(v, from, to, stats) => {
                let mut values: Vec<T> = vec![];
                let mut current = from;
                while current < to {
                    values.push(current.clone());
                    current = T::one() + &current;
                }

                let res = values
                    .into_iter()
                    .map(|index| {
                        vec![
                            vec![
                                TypedStatement::Declaration(v.clone()),
                                TypedStatement::Definition(
                                    TypedAssignee::Identifier(v.clone()),
                                    FieldElementExpression::Number(index).into(),
                                ),
                            ],
                            stats.clone(),
                        ]
                        .into_iter()
                        .flat_map(|x| x)
                    })
                    .flat_map(|x| x)
                    .flat_map(|x| self.fold_statement(x))
                    .collect();

                res
            }
            s => fold_statement(self, s),
        }
    }

    fn fold_function(&mut self, f: TypedFunction<'ast, T>) -> TypedFunction<'ast, T> {
        self.substitution = HashMap::new();
        for arg in &f.arguments {
            self.substitution.insert(arg.id.id.clone(), 0);
        }

        fold_function(self, f)
    }

    fn fold_name(&mut self, n: Identifier<'ast>) -> Identifier<'ast> {
        Identifier {
            version: self.substitution.get(&n).unwrap_or(&0).clone(),
            ..n
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zokrates_field::BN128;

    #[cfg(test)]
    mod statement {
        use super::*;

        #[test]
        fn for_loop() {
            // for field i in 2..5
            //		field foo = i

            // should be unrolled to
            // i_0 = 2
            // foo_0 = i_0
            // i_1 = 3
            // foo_1 = i_1
            // i_2 = 4
            // foo_2 = i_2

            let s = TypedStatement::For(
                Variable::field_element("i".into()),
                BN128::from(2),
                BN128::from(5),
                vec![
                    TypedStatement::Declaration(Variable::field_element("foo".into())),
                    TypedStatement::Definition(
                        TypedAssignee::Identifier(Variable::field_element("foo".into())),
                        FieldElementExpression::Identifier("i".into()).into(),
                    ),
                ],
            );

            let expected = vec![
                TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("i").version(0),
                    )),
                    FieldElementExpression::Number(BN128::from(2)).into(),
                ),
                TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("foo").version(0),
                    )),
                    FieldElementExpression::Identifier(Identifier::from("i").version(0)).into(),
                ),
                TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("i").version(1),
                    )),
                    FieldElementExpression::Number(BN128::from(3)).into(),
                ),
                TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("foo").version(1),
                    )),
                    FieldElementExpression::Identifier(Identifier::from("i").version(1)).into(),
                ),
                TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("i").version(2),
                    )),
                    FieldElementExpression::Number(BN128::from(4)).into(),
                ),
                TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("foo").version(2),
                    )),
                    FieldElementExpression::Identifier(Identifier::from("i").version(2)).into(),
                ),
            ];

            let mut u = Unroller::new();

            assert_eq!(u.fold_statement(s), expected);
        }

        #[test]
        fn definition() {
            // field a
            // a = 5
            // a = 6
            // a

            // should be turned into
            // a_0 = 5
            // a_1 = 6
            // a_1

            let mut u = Unroller::new();

            let s: TypedStatement<BN128> =
                TypedStatement::Declaration(Variable::field_element("a".into()));
            assert_eq!(u.fold_statement(s), vec![]);

            let s = TypedStatement::Definition(
                TypedAssignee::Identifier(Variable::field_element("a".into())),
                FieldElementExpression::Number(BN128::from(5)).into(),
            );
            assert_eq!(
                u.fold_statement(s),
                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("a").version(0)
                    )),
                    FieldElementExpression::Number(BN128::from(5)).into()
                )]
            );

            let s = TypedStatement::Definition(
                TypedAssignee::Identifier(Variable::field_element("a".into())),
                FieldElementExpression::Number(BN128::from(6)).into(),
            );
            assert_eq!(
                u.fold_statement(s),
                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("a").version(1)
                    )),
                    FieldElementExpression::Number(BN128::from(6)).into()
                )]
            );

            let e: FieldElementExpression<BN128> =
                FieldElementExpression::Identifier("a".into());
            assert_eq!(
                u.fold_field_expression(e),
                FieldElementExpression::Identifier(Identifier::from("a").version(1))
            );
        }

        #[test]
        fn incremental_definition() {
            // field a
            // a = 5
            // a = a + 1

            // should be turned into
            // a_0 = 5
            // a_1 = a_0 + 1

            let mut u = Unroller::new();

            let s: TypedStatement<BN128> =
                TypedStatement::Declaration(Variable::field_element("a".into()));
            assert_eq!(u.fold_statement(s), vec![]);

            let s = TypedStatement::Definition(
                TypedAssignee::Identifier(Variable::field_element("a".into())),
                FieldElementExpression::Number(BN128::from(5)).into(),
            );
            assert_eq!(
                u.fold_statement(s),
                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("a").version(0)
                    )),
                    FieldElementExpression::Number(BN128::from(5)).into()
                )]
            );

            let s = TypedStatement::Definition(
                TypedAssignee::Identifier(Variable::field_element("a".into())),
                FieldElementExpression::Add(
                    box FieldElementExpression::Identifier("a".into()),
                    box FieldElementExpression::Number(BN128::from(1)),
                )
                .into(),
            );
            assert_eq!(
                u.fold_statement(s),
                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("a").version(1)
                    )),
                    FieldElementExpression::Add(
                        box FieldElementExpression::Identifier(Identifier::from("a").version(0)),
                        box FieldElementExpression::Number(BN128::from(1))
                    )
                    .into()
                )]
            );
        }

        #[test]
        fn incremental_multiple_definition() {
            use crate::types::Type;

            // field a
            // a = 2
            // a = foo(a)

            // should be turned into
            // a_0 = 2
            // a_1 = foo(a_0)

            let mut u = Unroller::new();

            let s: TypedStatement<BN128> =
                TypedStatement::Declaration(Variable::field_element("a".into()));
            assert_eq!(u.fold_statement(s), vec![]);

            let s = TypedStatement::Definition(
                TypedAssignee::Identifier(Variable::field_element("a".into())),
                FieldElementExpression::Number(BN128::from(2)).into(),
            );
            assert_eq!(
                u.fold_statement(s),
                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_element(
                        Identifier::from("a").version(0)
                    )),
                    FieldElementExpression::Number(BN128::from(2)).into()
                )]
            );

            let s: TypedStatement<BN128> = TypedStatement::MultipleDefinition(
                vec![Variable::field_element("a".into())],
                TypedExpressionList::FunctionCall(
                    String::from("foo"),
                    vec![FieldElementExpression::Identifier("a".into()).into()],
                    vec![Type::FieldElement],
                ),
            );
            assert_eq!(
                u.fold_statement(s),
                vec![TypedStatement::MultipleDefinition(
                    vec![Variable::field_element(Identifier::from("a").version(1))],
                    TypedExpressionList::FunctionCall(
                        String::from("foo"),
                        vec![
                            FieldElementExpression::Identifier(Identifier::from("a").version(0))
                                .into()
                        ],
                        vec![Type::FieldElement],
                    )
                )]
            );
        }

        #[test]
        fn incremental_array_definition() {
            // field[2] a = [1, 1]
            // a[1] = 2

            // should be turned into
            // a_0 = [1, 1]
            // a_1 = [if 0 == 1 then 2 else a_0[0], if 1 == 1 then 2 else a_0[1]]

            let mut u = Unroller::new();

            let s: TypedStatement<BN128> =
                TypedStatement::Declaration(Variable::field_array("a".into(), 2));
            assert_eq!(u.fold_statement(s), vec![]);

            let s = TypedStatement::Definition(
                TypedAssignee::Identifier(Variable::field_array("a".into(), 2)),
                FieldElementArrayExpression::Value(
                    2,
                    vec![
                        FieldElementExpression::Number(BN128::from(1)),
                        FieldElementExpression::Number(BN128::from(1)),
                    ],
                )
                .into(),
            );

            assert_eq!(
                u.fold_statement(s),
                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_array(
                        Identifier::from("a").version(0),
                        2
                    )),
                    FieldElementArrayExpression::Value(
                        2,
                        vec![
                            FieldElementExpression::Number(BN128::from(1)),
                            FieldElementExpression::Number(BN128::from(1))
                        ]
                    )
                    .into()
                )]
            );

            let s: TypedStatement<BN128> = TypedStatement::Definition(
                TypedAssignee::ArrayElement(
                    box TypedAssignee::Identifier(Variable::field_array("a".into(), 2)),
                    box FieldElementExpression::Number(BN128::from(1)),
                ),
                FieldElementExpression::Number(BN128::from(2)).into(),
            );

            assert_eq!(
                u.fold_statement(s),
                vec![TypedStatement::Definition(
                    TypedAssignee::Identifier(Variable::field_array(
                        Identifier::from("a").version(1),
                        2
                    )),
                    FieldElementArrayExpression::Value(
                        2,
                        vec![
                            FieldElementExpression::IfElse(
                                box BooleanExpression::Eq(
                                    box FieldElementExpression::Number(BN128::from(1)),
                                    box FieldElementExpression::Number(BN128::from(0))
                                ),
                                box FieldElementExpression::Number(BN128::from(2)),
                                box FieldElementExpression::Select(
                                    box FieldElementArrayExpression::Identifier(
                                        2,
                                        Identifier::from("a").version(0)
                                    ),
                                    box FieldElementExpression::Number(BN128::from(0))
                                ),
                            ),
                            FieldElementExpression::IfElse(
                                box BooleanExpression::Eq(
                                    box FieldElementExpression::Number(BN128::from(1)),
                                    box FieldElementExpression::Number(BN128::from(1))
                                ),
                                box FieldElementExpression::Number(BN128::from(2)),
                                box FieldElementExpression::Select(
                                    box FieldElementArrayExpression::Identifier(
                                        2,
                                        Identifier::from("a").version(0)
                                    ),
                                    box FieldElementExpression::Number(BN128::from(1))
                                ),
                            ),
                        ]
                    )
                    .into()
                )]
            );
        }
    }
}
