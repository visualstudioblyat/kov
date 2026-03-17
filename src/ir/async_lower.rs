// async/await lowering: transform async functions into state machines
// async fn read() -> u32 { let x = uart.read().await; x + 1 }
// becomes a state machine struct with resume() method

use super::{Function, Op, Value};

#[derive(Debug)]
pub struct AsyncStateMachine {
    pub name: String,
    pub states: Vec<State>,
    pub state_type_size: u32,
}

#[derive(Debug)]
pub struct State {
    pub id: u32,
    pub body: Vec<StateOp>,
    pub transition: StateTransition,
}

#[derive(Debug)]
pub enum StateOp {
    Compute(Op),
    SaveLocal(String, u32), // save to state struct at offset
    LoadLocal(String, u32),
}

#[derive(Debug)]
pub enum StateTransition {
    Yield(u32),      // yield, resume at state N
    Complete(Value), // return final value
    Next(u32),       // goto state N (no yield)
}

// detect async functions and lower them to state machines
pub fn lower_async(func: &Function) -> Option<AsyncStateMachine> {
    // check if any block has an await point (Call that should yield)
    let has_await = func.blocks.iter().any(|b| {
        b.insts
            .iter()
            .any(|i| matches!(&i.op, Op::Call(name, _) if name.ends_with("_await")))
    });

    if !has_await {
        return None;
    }

    // split the function at each await point into states
    let mut states = Vec::new();
    let mut current_ops = Vec::new();
    let mut state_id = 0u32;

    for block in &func.blocks {
        for inst in &block.insts {
            if let Op::Call(name, _) = &inst.op {
                if name.ends_with("_await") {
                    // this is a yield point — end current state
                    states.push(State {
                        id: state_id,
                        body: current_ops.drain(..).map(StateOp::Compute).collect(),
                        transition: StateTransition::Yield(state_id + 1),
                    });
                    state_id += 1;
                    continue;
                }
            }
            current_ops.push(inst.op.clone());
        }
    }

    // final state
    states.push(State {
        id: state_id,
        body: current_ops.drain(..).map(StateOp::Compute).collect(),
        transition: StateTransition::Complete(Value(0)),
    });

    Some(AsyncStateMachine {
        name: func.name.clone(),
        states,
        state_type_size: 16, // fixed for now
    })
}

// simple round-robin executor that polls tasks
pub fn generate_executor_code(tasks: &[String]) -> String {
    let mut code = String::new();
    code.push_str("fn __executor__() {\n");
    code.push_str("    loop {\n");
    for task in tasks {
        code.push_str(&format!("        {}_poll();\n", task));
    }
    code.push_str("    }\n");
    code.push_str("}\n");
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_gen() {
        let code = generate_executor_code(&["read_sensor".into(), "blink_led".into()]);
        assert!(code.contains("read_sensor_poll"));
        assert!(code.contains("blink_led_poll"));
        assert!(code.contains("loop"));
    }
}
