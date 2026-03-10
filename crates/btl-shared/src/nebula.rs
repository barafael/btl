//! Procedural nebula background generation.
//!
//! Ported from the randomfart expression-tree approach: three independent
//! math expressions (one per color channel) are randomly generated from a
//! weighted grammar, compiled to stack-machine bytecode, and evaluated
//! per-pixel with an animated time parameter.

// ─── Bytecode VM ─────────────────────────────────────────────────────────────

/// Stack-machine opcode.
#[derive(Clone, Copy)]
pub enum Op {
    X,
    Y,
    T,
    Num(f32),
    Add,
    Mult,
    Sqrt,
    Abs,
    Sin,
    Mix,
}

/// Evaluate a compiled program at (x, y, t). All coordinates in [-1, 1].
pub fn eval_program(ops: &[Op], x: f32, y: f32, t: f32) -> f32 {
    let mut stack = [0f32; 32];
    let mut sp = 0usize;
    for &op in ops {
        match op {
            Op::X => {
                stack[sp] = x;
                sp += 1;
            }
            Op::Y => {
                stack[sp] = y;
                sp += 1;
            }
            Op::T => {
                stack[sp] = t;
                sp += 1;
            }
            Op::Num(n) => {
                stack[sp] = n;
                sp += 1;
            }
            Op::Abs => stack[sp - 1] = stack[sp - 1].abs(),
            Op::Sqrt => stack[sp - 1] = stack[sp - 1].abs().sqrt(),
            Op::Sin => stack[sp - 1] = (stack[sp - 1] * std::f32::consts::PI).sin(),
            Op::Add => {
                sp -= 1;
                stack[sp - 1] += stack[sp];
            }
            Op::Mult => {
                sp -= 1;
                stack[sp - 1] *= stack[sp];
            }
            Op::Mix => {
                sp -= 2;
                let w = ((stack[sp + 1] + 1.0) * 0.5).clamp(0.0, 1.0);
                stack[sp - 1] = stack[sp - 1] * (1.0 - w) + stack[sp] * w;
            }
        }
    }
    stack[0]
}

/// Map [-1, 1] value to [0, 255] byte.
pub fn channel(v: f32) -> u8 {
    (((v + 1.0) * 0.5).clamp(0.0, 1.0) * 255.0) as u8
}

// ─── Expression tree (private) ───────────────────────────────────────────────

enum Expr {
    X,
    Y,
    T,
    Num(f32),
    Add(Box<Expr>, Box<Expr>),
    Mult(Box<Expr>, Box<Expr>),
    Sqrt(Box<Expr>),
    Abs(Box<Expr>),
    Sin(Box<Expr>),
    Mix(Box<Expr>, Box<Expr>, Box<Expr>),
}

impl Expr {
    fn compile(&self, ops: &mut Vec<Op>) {
        match self {
            Expr::X => ops.push(Op::X),
            Expr::Y => ops.push(Op::Y),
            Expr::T => ops.push(Op::T),
            Expr::Num(n) => ops.push(Op::Num(*n)),
            Expr::Abs(e) => {
                e.compile(ops);
                ops.push(Op::Abs);
            }
            Expr::Sqrt(e) => {
                e.compile(ops);
                ops.push(Op::Sqrt);
            }
            Expr::Sin(e) => {
                e.compile(ops);
                ops.push(Op::Sin);
            }
            Expr::Add(a, b) => {
                a.compile(ops);
                b.compile(ops);
                ops.push(Op::Add);
            }
            Expr::Mult(a, b) => {
                a.compile(ops);
                b.compile(ops);
                ops.push(Op::Mult);
            }
            Expr::Mix(a, b, c) => {
                a.compile(ops);
                b.compile(ops);
                c.compile(ops);
                ops.push(Op::Mix);
            }
        }
    }

    fn generate(rng: &mut NebulaRng, depth: u32) -> Self {
        // Weights tuned for smooth flowing nebula backgrounds:
        // High sin/add/mix = undulations and smooth blends
        // No mod = no tiling artifacts
        const W_TERMINAL: u32 = 2;
        const W_ADD: u32 = 3;
        const W_MULT: u32 = 2;
        const W_SQRT: u32 = 1;
        const W_SIN: u32 = 3;
        const W_MIX: u32 = 2;
        const TOTAL: u32 = W_TERMINAL + W_ADD + W_MULT + W_SQRT + W_SIN + W_MIX;

        let roll = rng.next_u32() % TOTAL;

        if depth == 0 || roll < W_TERMINAL {
            return Self::terminal(rng);
        }
        let mut cursor = W_TERMINAL;

        cursor += W_ADD;
        if roll < cursor {
            return Expr::Add(
                Box::new(Self::generate(rng, depth - 1)),
                Box::new(Self::generate(rng, depth - 1)),
            );
        }
        cursor += W_MULT;
        if roll < cursor {
            return Expr::Mult(
                Box::new(Self::generate(rng, depth - 1)),
                Box::new(Self::generate(rng, depth - 1)),
            );
        }
        cursor += W_SQRT;
        if roll < cursor {
            return Expr::Sqrt(Box::new(Expr::Abs(Box::new(Self::generate(
                rng,
                depth - 1,
            )))));
        }
        cursor += W_SIN;
        if roll < cursor {
            return Expr::Sin(Box::new(Self::generate(rng, depth - 1)));
        }
        // Mix (remaining weight)
        Expr::Mix(
            Box::new(Self::generate(rng, depth - 1)),
            Box::new(Self::generate(rng, depth - 1)),
            Box::new(Self::generate(rng, depth - 1)),
        )
    }

    fn terminal(rng: &mut NebulaRng) -> Self {
        match rng.next_u32() % 7 {
            0 => Expr::Num(rng.next_f32() * 2.0 - 1.0),
            1 => Expr::X,
            2 => Expr::Y,
            3 => Expr::Abs(Box::new(Expr::X)),
            4 => Expr::Abs(Box::new(Expr::Y)),
            5 => Expr::Sqrt(Box::new(Expr::Add(
                Box::new(Expr::Mult(Box::new(Expr::X), Box::new(Expr::X))),
                Box::new(Expr::Mult(Box::new(Expr::Y), Box::new(Expr::Y))),
            ))),
            6 => Expr::T,
            _ => unreachable!(),
        }
    }
}

// ─── Simple xorshift64 RNG ──────────────────────────────────────────────────

struct NebulaRng(u64);

impl NebulaRng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn next_u32(&mut self) -> u32 {
        (self.next() & 0xFFFF_FFFF) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next() % 1_000_000) as f32 / 1_000_000.0
    }
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Three compiled bytecode programs (one per color channel).
pub struct NebulaPrograms {
    pub r: Vec<Op>,
    pub g: Vec<Op>,
    pub b: Vec<Op>,
}

/// Expression tree depth for nebula generation.
const NEBULA_DEPTH: u32 = 5;

/// Generate nebula programs from a seed. Deterministic: same seed = same nebula.
pub fn generate_nebula(seed: u64) -> NebulaPrograms {
    let mut rng = NebulaRng(seed);
    let r_expr = Expr::generate(&mut rng, NEBULA_DEPTH);
    let g_expr = Expr::generate(&mut rng, NEBULA_DEPTH);
    let b_expr = Expr::generate(&mut rng, NEBULA_DEPTH);

    let mut r = Vec::new();
    let mut g = Vec::new();
    let mut b = Vec::new();
    r_expr.compile(&mut r);
    g_expr.compile(&mut g);
    b_expr.compile(&mut b);

    NebulaPrograms { r, g, b }
}
