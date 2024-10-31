#include <assert.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <weval.h>
#include <wizer.h>

WIZER_DEFAULT_INIT();
WEVAL_DEFINE_GLOBALS();

enum Opcode {
    PushConst,
    Drop,
    Dup,
    GetLocal,
    SetLocal,
    Add,
    Sub,
    Print,
    Goto,
    GotoIf,
    Exit,
};

struct Inst {
    Opcode opcode;
    uint32_t imm;

    explicit Inst(Opcode opcode_) : opcode(opcode_), imm(0) {}
    Inst(Opcode opcode_, uint32_t imm_) : opcode(opcode_), imm(imm_) {}
};

#define OPSTACK_SIZE 32
#define LOCAL_SIZE 32

struct State {
    uint64_t opstack[OPSTACK_SIZE];
    uint64_t locals[LOCAL_SIZE];
};

uint32_t Interpret(const Inst* insts, uint32_t ninsts, State* state) {
    uint32_t pc = 0;
    uint32_t steps = 0;
    uint64_t* opstack = state->opstack;
    uint64_t* locals = state->locals;
    int sp = 0;

    weval::push_context(pc);
    while (true) {
        steps++;
        const Inst* inst = &insts[pc];
        pc++;
        weval::update_context(pc);
        switch (inst->opcode) {
        case PushConst: {
            if (sp + 1 > OPSTACK_SIZE) {
                return 0;
            }
            weval_push_stack(&opstack[sp++], inst->imm);
            break;
        }
        case Drop: {
            if (sp == 0) {
                return 0;
            }
            weval_pop_stack(&opstack[--sp]);
            break;
        }
        case Dup: {
            if (sp + 1 > OPSTACK_SIZE) {
                return 0;
            }
            if (sp == 0) {
                return 0;
            }
            uint64_t value = weval_read_stack(0, &opstack[sp - 1]);
            weval_push_stack(&opstack[sp++], value);
            break;
        }
        case GetLocal: {
            if (sp + 1 > OPSTACK_SIZE) {
                return 0;
            }
            if (inst->imm >= LOCAL_SIZE) {
                return 0;
            }
            uint64_t value = weval_read_local(inst->imm, &locals[inst->imm]);
            weval_push_stack(&opstack[sp++], value);
            break;
        }
        case SetLocal: {
            if (sp == 0) {
                return 0;
            }
            if (inst->imm >= LOCAL_SIZE) {
                return 0;
            }
            uint64_t value = weval_pop_stack(&opstack[--sp]);
            weval_write_local(inst->imm, &locals[inst->imm], value);
            break;
        }
        case Add: {
            if (sp < 2) {
                return 0;
            }
            uint64_t a = weval_pop_stack(&opstack[--sp]);
            uint64_t b = weval_pop_stack(&opstack[--sp]);
            weval_push_stack(&opstack[sp++], a + b);
            break;
        }
        case Sub: {
            if (sp < 2) {
                return 0;
            }
            uint64_t a = weval_pop_stack(&opstack[--sp]);
            uint64_t b = weval_pop_stack(&opstack[--sp]);
            weval_push_stack(&opstack[sp++], a - b);
            break;
        }
        case Print: {
            if (sp == 0) {
                return 0;
            }
            uint64_t value = weval_pop_stack(&opstack[--sp]);
            printf("%" PRIu64 "\n", value);
            break;
        }
        case Goto: {
            if (inst->imm >= ninsts) {
                return 0;
            }
            pc = inst->imm;
            weval::update_context(pc);
            break;
        }
        case GotoIf: {
            if (sp == 0) {
                return 0;
            }
            if (inst->imm >= ninsts) {
                return 0;
            }
            uint64_t value = weval_pop_stack(&opstack[--sp]);
            if (value != 0) {
                pc = inst->imm;
                weval::update_context(pc);
                continue;
            }
            break;
        }
        case Exit:
            goto out;
        }
    }
out:
    weval::pop_context();

    printf("Exiting after %d steps at PC %d.\n", steps, pc);
    return steps;
}

static const uint32_t kIters = 10000000;
// clang-format off
Inst prog[] = {
    Inst(PushConst, 0),
    Inst(Dup),
    Inst(PushConst, kIters),
    Inst(Sub),
    Inst(GotoIf, 6),
    Inst(Exit),
    Inst(PushConst, 1),
    Inst(Add),
    Inst(Goto, 1),
};
// clang-format on
static const uint32_t kExpectedSteps = 7 * kIters + 6;

typedef uint32_t (*InterpretFunc)(const Inst* insts, uint32_t ninsts,
                                  State* state);

WEVAL_DEFINE_TARGET(1, Interpret);

struct Func {
    const Inst* insts;
    uint32_t ninsts;
    InterpretFunc specialized;

    Func(const Inst* insts_, uint32_t ninsts_)
        : insts(insts_), ninsts(ninsts_), specialized(nullptr) {
        printf("ctor: ptr %p\n", &specialized);
        auto* req = weval::weval(
            &specialized, &Interpret, 1, 0,
            weval::SpecializeMemory<const Inst*>(insts, ninsts * sizeof(Inst)),
            weval::Specialize(ninsts), weval::Runtime<State*>());
        assert(req);
    }

    uint32_t invoke(State* state) {
        printf("Inspecting func ptr at: %p -> %p (size %lu)\n", &specialized,
               specialized, sizeof(specialized));
        if (specialized) {
            printf("Calling specialized function: %p\n", specialized);
            return specialized(insts, ninsts, state);
        }
        return Interpret(insts, ninsts, state);
    }
};

Func prog_func(prog, sizeof(prog) / sizeof(Inst));

int main(int argc, char** argv) {
    State* state = (State*)calloc(sizeof(State), 1);
    uint32_t steps = prog_func.invoke(state);
    assert(kExpectedSteps == steps);
    fflush(stdout);
}
