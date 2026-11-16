//! Fused instructions must not be seen: a program runs the same with them as
//! one instruction at a time, up to its errors and where each time slice ends.

use sb_core::bytecode::{Compiled, Op};
use sb_core::fuse::FastOp;
use sb_core::host::TextHost;
use sb_core::vm::{Status, Step, Vm};
use sb_core::watch::Watch;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

fn compiled(source: &str, fused: bool) -> Compiled {
    let host = TextHost::default();
    let mut program = sb_core::compile(source, &host).unwrap_or_else(|e| panic!("{source}: {e}"));
    if !fused {
        program.fast.clear();
    }
    program
}

/// What can be seen of a run in slices of `budget` instructions: where each
/// slice ended, how it ended, and the output.
fn run(source: &str, budget: u32, fused: bool) -> (Vec<String>, String) {
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled(source, fused));
    let mut seen = Vec::new();
    for _ in 0..200_000 {
        let result = vm.run(&mut host, budget);
        // An error holds an address, so not all of it can be compared.
        let shown = match &result {
            Ok(status) => format!("{status:?}"),
            Err(e) => format!("{e} {:?} {} {} {:?}", e.kind, e.line, e.col, e.detail),
        };
        let (executed, line, col) = (vm.executed(), vm.line(), vm.col());
        seen.push(format!("{shown} executed {executed} line {line} col {col}"));
        match result {
            Ok(Status::Ended) | Err(_) => return (seen, host.output),
            Ok(_) => {}
        }
    }
    panic!("{source}: did not end");
}

const PROGRAMS: &[&str] = &[
    // FOR in integers and reals, up and down, with TO and STEP that change.
    "FOR I=1 TO 5:?I:NEXT:?I",
    "FOR I=5 TO 1 STEP -2:?I:NEXT:?I",
    "FOR I=0 TO 1 STEP 0.25:?I:NEXT:?I",
    "FOR I#=1 TO 3:?I#:NEXT:?I#",
    "N=3:S=1:FOR I=0 TO N STEP S:N=N-1:S=S+1:?I:NEXT",
    "FOR I=1 TO 4 STEP I+1:?I:NEXT",
    "FOR I=1 TO I+1:?I:IF I>4 THEN BREAK\nNEXT",
    "FOR I%=2147483646 TO 2147483647:?I%:IF I%<0 THEN BREAK\nNEXT:?I%",
    "DEF F N\nFOR I=0 TO N:?I:NEXT\nEND\nF 3",
    "Z=0:FOR I=0 TO 1/Z\nNEXT",
    "S$=\"A\":FOR I=0 TO S$:NEXT",
    "S$=\"A\":FOR I=0 TO 3 STEP S$:NEXT",
    "T=1:S$=\"A\":FOR I=0 TO T\n?I\nT=S$\nNEXT\n?\"AFTER\"",
    "T=1:S$=\"A\":FOR I=0 TO 1 STEP T\n?I\nT=S$\nNEXT\n?\"AFTER\"",
    "T=2\nS$=\"A\":FOR I=0 TO 3:T=T-1:NEXT\n?I:FOR J=0 TO T:J=S$\nNEXT",
    // Arithmetic and comparisons of every kind, where they fuse and not.
    "A=1:B=2:C=A+B:?C:C=A-B:?C:C=A*B:?C:C=A/B:?C:C=A DIV B:?C:C=A MOD B:?C",
    "A=7:B=2:?A<<B:?A>>B:?A AND B:?A OR B:?A XOR B:?(A*2)+(B*3):?(A*2)-(B*3)",
    "A#=1.5:B%=2:?A#+B%:?B%+A#:?A#*B%:?A#/B%:?A# DIV B%:?A# MOD B%:?A#<<B%:?A# AND B%",
    "A%=2147483647:A%=A%+1:?A%:B%=-2147483648:B%=B%-1:?B%:C%=65536:C%=C%*C%:?C%",
    "A=0/1:?A==0:?A!=0:?A<1:?A<=0:?A>0:?A>=0:IF A==0 THEN ?\"Z\"\nIF A THEN ?\"NZ\"",
    "N#=1E308*10:M#=N#-N#:?M#==M#:?M#!=M#:?M#<1:?M#>1:IF M# THEN ?\"T\" ELSE ?\"F\"",
    "A$=\"X\":B$=\"Y\":?A$+B$:C$=A$+B$:?C$:?A$<B$:?A$==B$:?A$*3:D=2:?A$*D:IF A$<B$ THEN ?1",
    "A=1:IF A==1 && A<2 THEN ?\"AND\"\nIF A==2 || A<2 THEN ?\"OR\"\nWHILE A<4:A=A+1:WEND:?A",
    "X=(1+2)*(3+4):?X:Y=2:Z=3-(Y*2):?Z:Z=(Y*2)-3:?Z:W=Y-(Z*2)-(Y+Z):?W",
    "REPEAT:A=A+1:UNTIL A>=3:?A",
    // Errors in each shape, not at the start of a line.
    "A=1:B=0:C=A/B",
    "A=1:B=0\n?1:C=A/B:?2",
    "A=1:B=0\n?1:C=A DIV B:?2",
    "A=1:B=0\n?1:C=(A+1) MOD B:?2",
    "A=1:B=0\n?1:IF A/B THEN ?2",
    "A=1:B=0:D=0:C=D+(A/B)",
    "A=1:B$=\"X\"\n?1:C=A+B$:?2",
    "A=1:B$=\"X\"\n?1:C=B$+A:?2",
    "A=1:B$=\"X\"\n?1:C=(A+1)+B$:?2",
    "A=1:B$=\"X\"\n?1:C=B$+(A+1):?2",
    "A=1:B$=\"X\"\n?1:C=A-B$:?2",
    "A=1:B$=\"X\"\n?1:IF A<B$ THEN ?2",
    "B$=\"X\"\n?1:IF B$ THEN ?2",
    "B$=\"X\"\n?1:WHILE B$:WEND",
    "A#=1E300\n?1:C=A# AND 1:?2",
    "A#=1E300\n?1:C=A#<<1:?2",
    "VAR A\nDEF F OUT X\nEND\nF OUT A:?1:B=A+1",
    // Arrays: kinds, shapes, ranges, conversions.
    "DIM A[4]:A[1]=3:A[2]=A[1]+1:?A[2]:I=2:J=A[I]:?J:A[I]=J+1:?A[2]:?A[I]+A[1]",
    "DIM A%[4]:A%[1]=3.75:?A%[1]:I#=1.75:?A%[I#]:A%[I#]=9:?A%[1]:A%[2]=-1.5:?A%[2]",
    "DIM A#[4]:A#[1]=3:?A#[1]:I=1:?A#[I]",
    "DIM A[2,3]:A[1,2]=5:?A[1,2]:I=1:J=2:?A[I,J]:A[I,J]=6:?A[5]:?A[I*1,J*1]:A[I*1,J*1]=7:?A[5]",
    "DIM A$[3]:A$[1]=\"X\":?A$[1]:I=1:B$=A$[I]:?B$:A$[I]=\"Y\":?A$[1]:?A$[I]+\"Z\"",
    "S$=\"ABC\":?S$[1]:I=2:?S$[I]:S$[I]=\"XY\":?S$:C$=S$[0]:?C$",
    "DIM A[2]\n?1:?A[2]",
    "DIM A[2]\n?1:I=-1:?A[I]",
    "DIM A[2]\n?1:I=2:B=A[I]:?2",
    "DIM A[2]\n?1:I=2:A[I]=1:?2",
    "DIM A[2]\n?1:I=1E300:A[I]=1:?2",
    "DIM A[2]\n?1:I=0/1:J=1:A[I,J]=1:?2",
    "DIM A[2,2]\n?1:I=2:J=0:?A[I,J]:?2",
    "DIM A[2,2]\n?1:I=0:J=2:A[I,J]=1:?2",
    "DIM A[2,2]\n?1:I=3:?A[I]:I=4:?A[I]",
    "DIM A[2,2,2]\n?1:I=0:J=0:?A[I,J]",
    "DIM A%[2]\n?1:X=1E300:A%[0]=X:?2",
    "DIM A%[2]\n?1:X$=\"S\":A%[0]=X$:?2",
    "DIM A$[2]\n?1:A$[0]=1:?2",
    "DIM A[2]\n?1:I$=\"X\":?A[I$]",
    "A=1\n?1:I=0:?A[I]",
    "A=1\n?1:I=0:A[I]=1",
    "VAR A[2],B[2]\nB=A:B[0]=1:?A[0]:C=A[0]+B[0]:?C",
    // DEFs, locals, GOSUB, READ, the values the stack holds meanwhile.
    "DEF ADD(A,B)\nVAR C=A+B\nRETURN C\nEND\n?ADD(1,2)+ADD(3,4):?ADD(1.5,2)*2",
    "DEF TWO A,B OUT X,Y\nX=A+B:Y=A*B\nEND\nTWO 2,3 OUT P,Q:?P:?Q",
    "DEF F(A)\nIF A<=1 THEN RETURN 1\nRETURN A*F(A-1)\nEND\n?F(10)",
    "DEF F(A)\nRETURN A+B$\nEND\nVAR B$=\"X\":?F(1)",
    "DEF F A$\nA$=A$+\"Y\":?A$\nEND\nS$=\"X\":F S$:?S$",
    "GOSUB @S:?A:END\n@S:A=A+1:IF A<3 THEN GOSUB @S\nRETURN",
    "READ A,B$,C:?A:?B$:?C:DATA 1,\"S\",2.5",
    "A=1:B=2:SWAP A,B:?A:?B:INC A:DEC B:?A:?B:INC A,2:?A",
    "S$=\"A\":INC S$,\"B\":?S$:T$=S$:INC T$,\"C\":?S$",
    "?ABS(-1)+1:?MAX(1,2)*2:?FLOOR(1.5)+CEIL(1.5):?LEN(\"AB\")+1:?SIN(0)",
    "DIM A[3]:?LEN(A)+1:A[1]=MAX(1,2):?A[1]:PUSH A,5:?A[3]:?POP(A)",
    "X=-1:?-X:?!X:?NOT X:Y=-X+1:?Y",
    "ON 1 GOTO @A,@B\n@A:?1\n@B:?2\nFOR I=0 TO 2:ON I GOSUB @C,@D:NEXT:END\n@C:?3:RETURN\n@D:?4:RETURN",
    "A=1:GOTO @L\nA=2\n@L:?A:B=A+1:?B",
    // ON GOTO and ON GOSUB: in range, out of range, from a variable, an
    // expression and an array, in a DEF, and each way the selector fails.
    ON_GOTO,
    ON_GOSUB,
    "FOR I=-1 TO 3:ON I+1 GOSUB @A,@B:NEXT:END\n@A:?\"A\":RETURN\n@B:?\"B\":RETURN",
    ON_ARRAY,
    ON_REAL,
    "DEF F N\nVAR I\nFOR I=0 TO N:ON I GOSUB @A,@B:NEXT\nEND\nF 3:END\n@A:?\"A\":RETURN\n@B:?\"B\":RETURN",
    ON_IN_DEF,
    "X$=\"A\"\n?1:ON X$ GOTO @A\n@A:?2",
    "X=1E300\n?1:ON X GOTO @A\n@A:?2",
    "DIM K%[2]\n?1:I=2:ON K%[I] GOTO @A\n@A:?2",
    "DIM K$[2]\n?1:I=0:ON K$[I] GOTO @A\n@A:?2",
    "?1:RETURN",
    "GOSUB @S:?\"OK\":END\n@S:RESTORE \"PUSH\":RETURN",
    "DEF F\nGOSUB @S\n?\"F\"\nEND\nF:?\"OUT\":END\n@S:?\"S\":RETURN",
    // CASE on numbers and strings, WHENs sharing a body, OTHERWISE.
    CASE_NUMBERS,
    "FOR A#=0 TO 2 STEP 0.5\nCASE A#\nWHEN 1:?\"ONE\"\nWHEN 1.5:?\"HALF\"\nENDCASE\nNEXT",
    CASE_STRINGS,
    "S$=\"B\"\n?1:CASE S$\nWHEN 1:?2\nENDCASE",
    "A=1\n?1:CASE A\nWHEN \"A\":?2\nENDCASE",
    // Arrays indexed by arrays.
    NESTED_INT,
    NESTED_REAL,
    "DIM R%[4],K%[4]\n?1:K%[0]=4:I=0:?R%[K%[I]]",
    "DIM R%[4],K%[4]\n?1:K%[0]=-1:I=0:R%[K%[I]]=1",
    "DIM R%[4],K%[4]\n?1:I=4:?R%[K%[I]]",
    "DIM R%[4],K%[4]\n?1:I=4:R%[K%[I]]=1",
    "DIM R$[2],K%[2]:R$[0]=\"S\":I=0:?R$[K%[I]]:R$[K%[I]]=\"T\":?R$[0]",
    "DIM R%[2],K$[2]\n?1:I=0:?R%[K$[I]]",
    "DIM R%[2],K%[2]\n?1:X$=\"A\":I=0:R%[K%[I]]=X$",
    // DEF calls: OUT, recursion, strings and arrays, each way they fail.
    DEF_OUT,
    DEF_RECURSIVE,
    DEF_STRINGS_AND_ARRAYS,
    "DEF F(A)\nRETURN A+1\nEND\nF 1 OUT X:?X:DEF G A OUT B\nB=A*3\nEND\n?G(2)+1",
    "DEF F(A)\nIF A THEN RETURN\nRETURN 5\nEND\n?F(0):?TYPEOF(F(1))",
    "DEF F A OUT B,C\nB=A\nEND\nF 1 OUT X,Y:?X:?TYPEOF(Y)",
    "DEF F(A,B)\nRETURN A*10+B\nEND\nDEF G(X)\n?X\nRETURN X\nEND\n?F(G(1),G(2))",
    "DEF F()\nGOSUB @S\nRETURN 1\n@S\nRETURN 2\nEND\n?F():GOSUB @T:END\n@T:?F():RETURN",
    "DEF F\nGOSUB @S\n?\"F\"\nRETURN\n@S:?\"S\":RETURN\nEND\nF:F:?\"OUT\"",
    "DEF F(A)\nVAR B=0\nRETURN A/B\nEND\n?1:C=F(1)",
    "DEF G A\n?A:A=A+\"X\"\nEND\n?1:G 1",
    "DEF F(A)\nRETURN A\nEND\n?1:B=F(1,2)",
    "DEF F(A)\nRETURN A\nEND\n?1:B=F()",
    "DEF F OUT A,B\nA=1\nEND\n?1:F OUT X",
    "DEF F OUT A,B\nA=1\nEND\n?1:X=F()",
    "DEF F A\nEND\n?1:X=F(1)",
    "DEF F(A)\nRETURN A\nEND\n?1:F 1 OUT X,Y",
    "DEF F\nEND\n?1:F 1",
    "DEF F\nRETURN\nEND\nF:?1:RETURN",
    "DEF F()\nRETURN 2\nEND\nFOR I=0 TO 9 STEP F():?I:NEXT",
    "DEF F()\nRETURN 4\nEND\nFOR I=0 TO F():?I:NEXT",
    // Builtins called in fused steps: on numbers, strings and arrays, and
    // each way they fail there.
    "FOR I=-2 TO 2:?ABS(I);SGN(I);INT(I/3);FLOOR(I/3);CEIL(I/3);ROUND(I/3):NEXT",
    "X=0.5:?SIN(X)+COS(X)*TAN(X):?SQR(X)*EXP(X)+LOG(2)+LOG(8,2):?POW(2,X):?ATAN(X,2)",
    "A=3:B=7:?MIN(A,B)*MAX(A,B,1.5):?MIN(A):DIM M[3]:M[1]=-4:?MAX(M)+MIN(M)",
    "S$=\"AB\":?LEN(S$)+ASC(S$):DIM Q[5]:?LEN(Q)*2:?DEG(1)+RAD(90):?FLOAT(3)/2",
    "X=-1\n?1:Y=SQR(X):?2",
    "X=0\n?1:Y=LOG(X)+1:?2",
    "S$=\"\"\n?1:Y=ASC(S$)+1:?2",
    "X=1E300\n?1:Y=INT(X):?2",
    "X$=\"A\"\n?1:Y=ABS(X$):?2",
    "DIM M$[2]\n?1:Y=MAX(M$):?2",
    "X=2\n?1:Y=ASIN(X):?2",
    // FORs whose TO or STEP is the counter, and counters that overflow.
    "J=2:FOR K=0 TO 9 STEP J:J=J+1:?K:NEXT:FOR K=0 TO 3 STEP K+1:?K:NEXT",
    "I=3:FOR I=0 TO I:?I:IF I>5 THEN BREAK\nNEXT",
    "FOR I%=2147483640 TO 2147483647 STEP 3:?I%:NEXT:?I%",
    "FOR I=5 TO 1 STEP -1:?I:NEXT:FOR I=1 TO 0:?\"NO\":NEXT:?I",
    // Chains: statements of several operators, with each operand kind, the
    // result read and written in one, and every place a chain ends.
    "A=3:B=4:C=(A+B)*(A-B):?C:A=A+A:?A:A=A*A-A:?A:B=(B+1)*(B+2)-B:?B",
    "A%=7:B%=2:FOR I=1 TO 4:A%=(A%+I*3) AND 1023:B%=B% XOR (A%<<1):NEXT:?A%;B%",
    "X#=1.5:DX#=0.25:T#=2:FOR I=1 TO 3:X#=X#+DX#*T#:Y#=X#*X#-X#/T#:NEXT:?X#;Y#",
    "DIM A[4]:A[0]=1:A[1]=0:A[A[0]]=A[0]+A[1]:?A[1]:A[A[1]+1]=A[A[0]]*2+1:?A[2]",
    "DIM A[4]:A=2:A[A]=A:?A[2]",
    "DIM M#[4,4]:FOR I=0 TO 15:M#[I AND 3,(I>>2) AND 3]=M#[(I+1) AND 3,I AND 3]*0.5+I:NEXT:?M#[1,2]",
    "DIM M%[8]:FOR I=0 TO 20:M%[I AND 7]=(M%[(I*3) AND 7]+I) AND 255:NEXT:?M%[3];M%[5]",
    "X=3:Y=4:IF X*X+Y*Y==25 THEN ?\"R\"\nIF X+1<Y-1 THEN ?\"N\" ELSE ?\"E\"",
    "X=3:IF X>1 && X*2<7 THEN ?1\nIF X<1 || X*2>5 THEN ?2\nIF !(X-3) THEN ?3",
    "X=2:WHILE X*X<100:X=X*X+1:WEND:?X:REPEAT:X=X-7:UNTIL X*2<0:?X",
    "X=2:?(X+1)*(X+2);(X*3) MOD 4;-X*2;NOT X+1;!X*3",
    "X=0.5:Y=SQR(X*4)+SIN(X-0.5)*2:?Y:Z=MAX(X+1,X*3,2)-MIN(X,X/2):?Z",
    "A%=2147483647:B%=A%+1-1:?B%;TYPEOF(B%):C=-2147483648:D=C-1+1:?D;TYPEOF(D)",
    "A=65536:B=A*A+1:?B:C=46341*46341-1:?C:D=-(A*A):?D",
    "A=7:B=0:C=0\n?1:C=(A+1)/B+1:?2",
    "A=7:B=0\n?1:C=(A+1) MOD B+1:?2",
    "A=1:B$=\"X\"\n?1:C=(A+1)*2+B$:?2",
    "A=1:B$=\"X\"\n?1:C=B$+(A*2):?2",
    "A$=\"X\":B$=\"Y\":C$=A$+B$+A$:?C$:IF A$+B$==\"XY\" THEN ?\"S\"",
    "DIM A[2]\n?1:I=1:A[I+1]=I*2:?2",
    "DIM A[2]\n?1:I=1:B=A[I*2-3]+1:?2",
    "DIM A%[2]\n?1:X=1E300:A%[0]=X*2:?2",
    "S$=\"ABC\":I=1:C$=S$[I+1]:?C$:S$[I-1]=\"Z\":?S$",
    "DEF F(A,B)\nRETURN (A+B)*(A-B)\nEND\n?F(3,2)+F(1,1)*2",
    "DEF G A OUT B\nB=A*A+A\nEND\nG 3 OUT X:?X",
    "A=1\nGOTO @L\n@M:A=A+1\n@L:B=(A+2)*(A+3):?B:IF A<3 GOTO @M",
    // Chains of several statements, failing in a later one.
    "A=1:B$=\"X\"\n?1:C=A*2+1:D=C*C-A:E=(A+1)*B$:?2",
    "A=1:B=0\n?1:C=A*2+1:D=C*C-A:E=(A+1)/B:?2",
    "DIM M[4]:A=2\n?1:M[A-1]=A*A:M[A+1]=M[A-1]+A:M[A+9]=1:?2",
    "DIM M[4]:FOR I=0 TO 3:X=I*2+1:Y=X*X-I:M[I]=X+Y:NEXT:?M[3]",
    "A=1:B=2:C=(A+B)*(A+B*(A+B*(A+B*(A+B*(A+B*(A+B*(A+B*(A+B)))))))):?C",
    // Constants carried in entries and steps, on either side, against every
    // kind of value, where they overflow and where they fail.
    CONSTANTS,
    CONSTANTS_IN_DEF,
    "A%=2147483647:A%=A%+1:?A%:B%=-2147483648:B%=B%-1:?B%:C%=65536:C%=C%*65536:?C%",
    "A%=2147483647:A%=(A%+1)*2:?A%:B%=-2147483648:B%=(B%-1)*1:?B%:C=46341:C=C*46341+0:?C",
    "X#=0.5:X#=X#+0.25:?X#:X#=X#+1:?X#:Y=3:Y=Y+0.5:?Y;TYPEOF(Y):Z#=0.1:Z#=Z#*3+0.5:?Z#",
    "N#=1E308*10:M#=N#-N#:?1+M#;M#+1;2*M#;M#*2;M#-1;1 AND M#;M# OR 2;M#==1;1==M#",
    "N#=1E308*10:?N#+1;1+N#;N#*2;-1*N#;N#-1E308:IF N#>1 THEN ?\"BIG\"",
    "X#=1E300:?1:Y=X# AND 1:?Y",
    "X#=1E300:?1:Y=(X#*2) AND 1:?Y",
    "X#=2.5:?3 AND X#;X# AND 3;X# OR 1;X# XOR 7;X#<<1;(X#*2)>>1;X# MOD 2;(X#+1) MOD 2",
    "A=7:?(A+1) MOD 0",
    "A=7:B=(A+1) DIV 0",
    "A=7:B=A MOD 0",
    "S$=\"Q\":?1:S$=S$+1:?2",
    "S$=\"Q\":?1:T$=S$*2:?T$:U$=(S$+\"R\")*2:?U$:V$=2*S$",
    "S$=\"Q\":?1:IF S$==1 THEN ?2",
    "S$=\"Q\":?1:IF S$<1 THEN ?2",
    "S$=\"Q\":?1:B=(S$+\"R\")-1:?2",
    "DIM A[2]:?1:A=A+1",
    "DIM A[2]:?1:IF A>=0 THEN ?2",
    "A=5:B=10-A:C=2-(A*2):D=100/(A+1):E=7 MOD (A-1):F=1<<(A-3):G=3<A:H=(A*2)<4:?B;C;D;E;F;G;H",
    "K=0:IF 0==K THEN ?\"Z\"\nIF 1!=K THEN ?\"N\"\nIF K+1==1 THEN ?\"O\"\nIF (K+1)*2>=2 THEN ?\"T\"",
];

/// Constants on both sides of each operator, in entries and in chains, the
/// variables changing kind as they go.
const CONSTANTS: &str = "\
FOR I=0 TO 5
 IF I==3 THEN A=\"S\" ELSE A=I+0.5*(I AND 1)
 IF I==3 THEN ?\"STRING\":CONTINUE
 B=A+1:C=A-2:D=A*3:E=1+A:F=2*A:?B;C;D;E;F
 B=(A+1)*2:C=(A-2)*3:D=(A*3) AND 7:E=(A+1) OR 8:F=(A*2) XOR 5:?B;C;D;E;F
 B=(A*2)<<2:C=(A*4)>>1:D=(A+9) MOD 4:E=(A+9) DIV 2:F=(A+1)/4:?B;C;D;E;F
 B=A+0.25:C=A-0.5:D=A*1.5:E=(A+1)/0.5:F=(A+1) MOD 1.5:?B;C;D;E;F
 IF A==2 THEN ?\"EQ\"
 IF A!=2 THEN ?\"NE\"
 IF A<2 THEN ?\"LT\"
 IF A<=2 THEN ?\"LE\"
 IF A>2 THEN ?\"GT\"
 IF A>=2 THEN ?\"GE\"
 IF A*2==4 THEN ?\"EQ2\"
 IF A*2!=4 THEN ?\"NE2\"
 IF A*2<4 THEN ?\"LT2\"
 IF A*2<=4 THEN ?\"LE2\"
 IF A*2>4 THEN ?\"GT2\"
 IF A*2>=4 THEN ?\"GE2\"
 IF A*2>=4 || A<1 THEN ?\"OR2\"
 X#=A:X#=X#+0.75:Y%=A:Y%=Y%+1:Y%=Y%*2:Y%=Y%-3:?X#;Y%
NEXT
";
const CONSTANTS_IN_DEF: &str = "\
DEF F N
 VAR A,B,C
 A=N:B=A+1:C=(A*3) AND 5
 IF C>=2 THEN B=B-1
 IF (A+1)*2<6 THEN C=C+10
 ?A;B;C
END
VAR A=100
FOR I=0 TO 4:F I:F I+0.5:NEXT
?A
";

const DEF_OUT: &str = "\
DEF TWO A,B OUT X,Y
 X=A+B:Y=A*B
END
DEF ONE A OUT X
 X=A-1
END
FOR I=0 TO 3
 TWO I,2 OUT P,Q:ONE P OUT R
 ?P;Q;R;ONE(Q)
NEXT
";
const DEF_RECURSIVE: &str = "\
DEF FIB N OUT R
 VAR A,B
 IF N<2 THEN R=N:RETURN
 FIB N-1 OUT A:FIB N-2 OUT B:R=A+B
END
DEF FACT(N)
 IF N<=1 THEN RETURN 1
 RETURN N*FACT(N-1)
END
FIB 12 OUT X:?X:?FACT(10)
";
const DEF_STRINGS_AND_ARRAYS: &str = "\
DEF CAT(A$,B$)
 VAR C$=A$+B$
 RETURN C$
END
DEF SUM(K)
 VAR I,S
 FOR I=0 TO LEN(K)-1:S=S+K[I]:NEXT
 RETURN S
END
DEF BUMP K,N OUT T$
 K[0]=K[0]+N:T$=STR$(K[0])
END
DIM K[3]:K[1]=2:K[2]=3
FOR I=0 TO 2
 BUMP K,I OUT T$
 ?CAT(T$,\"!\");SUM(K)
NEXT
";

const ON_GOTO: &str = "\
FOR I=-2 TO 3
 ON I GOTO @A,@B,@C
 ?\"NONE\";I:GOTO @E
 @A:?\"A\":GOTO @E
 @B:?\"B\":GOTO @E
 @C:?\"C\"
 @E
NEXT
";
const ON_GOSUB: &str =
    "FOR I=-1 TO 3:ON I GOSUB @A,@B:?\"N\";I:NEXT:END\n@A:?\"A\":RETURN\n@B:?\"B\":RETURN";
const ON_ARRAY: &str = "DIM K%[3]:K%[1]=1:K%[2]=7:FOR I=0 TO 2:ON K%[I] GOTO @A,@B\n?\"N\":GOTO @E\n@A:?\"A\":GOTO @E\n@B:?\"B\"\n@E:NEXT";
const ON_REAL: &str = "X#=1.75:ON X# GOTO @A,@B\n@A:?\"A\"\n@B:?\"B\"";
const ON_IN_DEF: &str = "DEF F N\nON N GOTO @A\n?\"N\":RETURN\n@A:?\"A\"\nEND\nF 0:F 1";
const CASE_NUMBERS: &str = "FOR A=0 TO 4\nCASE A\nWHEN 0:WHEN 1:?\"A\"\nWHEN 2:?\"B\"\nWHEN 3.0:?\"C\"\nOTHERWISE:?\"X\"\nENDCASE\nNEXT";
const CASE_STRINGS: &str = "S$=\"B\"\nCASE S$\nWHEN \"A\":?1\nWHEN \"B\":?2\nENDCASE\nCASE S$+\"C\"\nWHEN \"BC\":?3\nENDCASE";
const NESTED_INT: &str =
    "DIM R%[4],K%[4]:K%[1]=3:R%[3]=9:I=1:?R%[K%[I]]:R%[K%[I]]=5:?R%[3]:V=R%[K%[I]]+1:?V";
const NESTED_REAL: &str = "DIM R#[4],K#[4]:K#[1]=2.5:R#[2]=1.5:I=1:?R#[K#[I]]:R#[K#[I]]=7:?R#[2]";

/// What ON, CASE and indexes by arrays print, fused.
#[test]
fn on_case_and_nested_indexes_print_what_they_should() {
    for (source, expected) in [
        (ON_GOTO, "NONE-2\nNONE-1\nA\nB\nC\nNONE3\n"),
        (ON_GOSUB, "N-1\nA\nN0\nB\nN1\nN2\nN3\n"),
        (ON_ARRAY, "A\nB\nN\n"),
        (ON_REAL, "B\n"),
        (ON_IN_DEF, "A\nN\n"),
        (CASE_NUMBERS, "A\nA\nB\nC\nX\n"),
        (CASE_STRINGS, "2\n3\n"),
        (NESTED_INT, "9\n5\n6\n"),
        (NESTED_REAL, "1.5\n7\n"),
    ] {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        assert_eq!(
            vm.run(&mut host, 1_000_000).unwrap(),
            Status::Ended,
            "{source}"
        );
        assert_eq!(host.output, expected, "{source}");
    }
}

/// What DEF calls print, fused.
#[test]
fn def_calls_print_what_they_should() {
    for (source, expected) in [
        (DEF_OUT, "201-1\n3221\n4433\n5645\n"),
        (DEF_RECURSIVE, "144\n3628800\n"),
        (DEF_STRINGS_AND_ARRAYS, "0!5\n1!6\n3!8\n"),
    ] {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        assert_eq!(
            vm.run(&mut host, 1_000_000).unwrap(),
            Status::Ended,
            "{source}"
        );
        assert_eq!(host.output, expected, "{source}");
    }
}

/// DEF calls fill the stack as without fusing.
#[test]
fn def_calls_overflow_as_without_fusing() {
    for program in [
        "DEF F(N)\nRETURN F(N+1)\nEND\n?1:A=F(0)",
        "DEF F N\nF N+1\nEND\n?1:F 0",
        "DEF F N OUT R\nVAR A$=\"S\"\nF N+1 OUT R\nEND\n?1:F 0 OUT X",
        "DEF F(N)\nGOSUB @S\nRETURN 0\n@S\nA=F(N+1)\nRETURN\nEND\n?1:A=F(0)",
    ] {
        for budget in [5, 7, 1_000_000] {
            let fused = run(program, budget, true);
            assert!(
                fused.0.last().unwrap().contains("StackOverflow"),
                "{fused:?}"
            );
            assert_eq!(fused, run(program, budget, false), "{program} in {budget}");
        }
    }
}

/// DEF calls and returns within the slot are fused.
#[test]
fn def_calls_are_fused() {
    let program = compiled(
        "DEF F(A)\nRETURN A+1\nEND\nDEF G A OUT B\nB=A\nEND\nFOR I=0 TO 3:G F(I) OUT C:NEXT",
        true,
    );
    let calls = program
        .fast
        .iter()
        .filter(|f| matches!(f.op, FastOp::CallDef { .. }))
        .count();
    assert_eq!(calls, 2);
    // F(I) reads its argument where it is.
    assert!(
        (program.fast.iter()).any(|f| matches!(f.op, FastOp::CallArgs { argc: 1, .. })),
        "{:?}",
        program.fast
    );
    for op in [FastOp::ReturnValue, FastOp::Return] {
        assert!(program.fast.iter().any(|f| f.op == op), "{op:?}");
    }
}

/// GOSUBs fill the stack as without fusing, which takes longer slices than
/// [`programs_run_the_same_fused_or_not`] uses.
#[test]
fn gosubs_overflow_as_without_fusing() {
    for program in [
        "?1:GOSUB @S\n@S:GOSUB @S",
        "?1:I=0:ON I GOSUB @S\n@S:ON I GOSUB @S",
    ] {
        for budget in [5, 7, 1_000_000] {
            let fused = run(program, budget, true);
            assert!(
                fused.0.last().unwrap().contains("StackOverflow"),
                "{fused:?}"
            );
            assert_eq!(fused, run(program, budget, false), "{program} in {budget}");
        }
    }
}

/// None of these leaves anything to `step` but DIM, the end of the program
/// and the FOR test that only stands in a larger entry.
#[test]
fn on_case_and_nested_indexes_are_fused() {
    for source in [
        "FOR I=0 TO 3:ON I GOTO @A,@B\nA=1\n@A:B=1\n@B:NEXT",
        "FOR I=0 TO 9:ON I GOSUB @S:NEXT:END\n@S:RETURN",
        "FOR A=0 TO 4\nCASE A\nWHEN 1:B=1\nWHEN 2:B=2\nENDCASE\nNEXT",
        "DIM R%[4],K%[4]:FOR I=0 TO 3:R%[K%[I]]=R%[K%[I]]+1:NEXT",
    ] {
        let program = compiled(source, true);
        let slow: Vec<_> = program
            .fast
            .iter()
            .zip(&program.code)
            .filter(|(f, op)| {
                f.op == FastOp::Slow && !matches!(op, Op::End | Op::ForTest | Op::NewArray { .. })
            })
            .map(|(_, op)| op)
            .collect();
        assert!(slow.is_empty(), "{source}: {slow:?}");
    }
}

#[test]
fn programs_run_the_same_fused_or_not() {
    for program in PROGRAMS {
        for budget in [1, 2, 3, 5, 7, 13, 100, 1_000_000] {
            let fused = run(program, budget, true);
            let plain = run(program, budget, false);
            assert_eq!(fused, plain, "{program} in slices of {budget}");
        }
    }
}

/// Calls keep their arguments on the stack while they run. Whatever comes of a
/// call, the stack and the error must be as if they had been taken off first.
#[test]
fn calls_leave_the_stack_as_it_should_be() {
    for (source, expected) in [
        ("A=1+ABS(-2)*MAX(3,4):?A", "9\n"),
        (
            "DEF F(A,B$)\nVAR C$=B$*A\nRETURN C$\nEND\n?F(2,\"X\")+F(1,\"Y\")",
            "XXY\n",
        ),
        ("DEF F(A)\nRETURN A+1\nEND\n?1+F(F(1))*2:?F(2)", "7\n3\n"),
        ("DEF G *\n?DEFARGC()\n?DEFARG(1)\nEND\nG 1,2,3", "3\n2\n"),
        (
            "DEF H * OUT *\nFOR I=0 TO DEFOUTC()-1:DEFOUT I,I+DEFARG(0):NEXT\nEND\nH 5 OUT A,B:?A:?B",
            "5\n6\n",
        ),
        ("DEF K OUT A,B\nA=1\nEND\nK OUT P,Q:?P:?TYPEOF(Q)", "1\n0\n"),
        (
            "DEF M A,B$ OUT C\nC=A+LEN(B$)\nEND\nM 1,\"AB\" OUT D:?D",
            "3\n",
        ),
        (
            "DEF R(N)\nVAR L$=\"S\":L$=L$*N\nRETURN LEN(L$)\nEND\n?R(2)+R(3)",
            "5\n",
        ),
    ] {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        assert_eq!(
            vm.run(&mut host, 1_000_000).unwrap(),
            Status::Ended,
            "{source}"
        );
        assert_eq!(host.output, expected, "{source}");
    }
}

#[test]
fn failed_calls_name_what_they_always_named() {
    use sb_core::error::ErrorKind as E;
    for (source, kind, line) in [
        ("A=1\n?MID$(\"X\")", E::WrongNumberOfArguments, 2),
        ("A=1\n?ABS(\"X\")", E::TypeMismatch, 2),
        ("A=1\nB=SQR(-1)", E::OutOfRange, 2),
        (
            "DEF F(A)\nRETURN A\nEND\nC=1\nB=F(1,2)",
            E::WrongNumberOfArguments,
            5,
        ),
        ("DEF F A\nEND\nC=1\nB=F(1)", E::IllegalFunctionCall, 4),
        (
            "DEF F(A)\nRETURN A\nEND\nC=1\nF 1 OUT P,Q",
            E::IllegalFunctionCall,
            5,
        ),
        (
            "DEF F OUT A,B\nEND\nC=1\nB=F()",
            E::WrongNumberOfOutArguments,
            4,
        ),
        (
            "DEF F(A)\nRETURN F(A+1)\nEND\nC=1\nB=F(1)",
            E::StackOverflow,
            2,
        ),
    ] {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        let e = vm.run(&mut host, 100_000_000).unwrap_err();
        assert_eq!((e.kind, e.line), (kind, line), "{source}");
    }
}

/// A call that asks for more memory than the last count allowed runs again after
/// counting, and finds its arguments still there.
#[test]
fn a_call_runs_again_after_counting_memory() {
    let source = "FOR I=0 TO 40:A%=ARRAY%(4000000):A%[0]=I:NEXT:?LEN(A%):?A%[0]";
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled(source, true));
    loop {
        match vm.run(&mut host, 1_000_000).unwrap() {
            Status::Ended => break,
            _ => continue,
        }
    }
    assert_eq!(host.output, "4000000\n40\n");
}

/// Variables the fused code reads are the same ones the rest of the program and
/// the debugger see, constants being kept out of their view.
#[test]
fn constants_are_not_variables() {
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled("A=5:B=A+7:C#=B*2.5", true));
    vm.run(&mut host, 1_000_000).unwrap();
    let mut names: Vec<String> = vm.globals().map(|(name, _)| name).collect();
    names.sort();
    assert_eq!(names, ["A", "B", "C#"]);
    assert!(matches!(vm.global("C#"), Some(sb_core::value::Value::Real(x)) if *x == 30.0));
    assert!(!vm.program().constants.is_empty());
}

/// Constants beside an operator are carried in the entry or step, from
/// either side where the sides can change places.
#[test]
fn constants_are_carried_in_entries_and_steps() {
    use sb_core::fuse::Micro;
    let source = "FOR I=0 TO 3:A=A+1:B=B-2:C=3*C:D#=D#+0.5\nIF A<5 THEN ?A\n\
                  E=(A+1)*2:F#=(D#-0.5)*1.5:IF A*2>=4 THEN ?E\nNEXT";
    let program = compiled(source, true);
    let ops: Vec<FastOp> = program.fast.iter().map(|f| f.op).collect();
    let has = |f: &dyn Fn(&FastOp) -> bool| ops.iter().any(f);
    assert!(
        has(&|o| matches!(o, FastOp::AddStoreInt(_, 1, _))),
        "{ops:?}"
    );
    assert!(
        has(&|o| matches!(o, FastOp::SubStoreInt(_, 2, _))),
        "{ops:?}"
    );
    assert!(
        has(&|o| matches!(o, FastOp::MulStoreInt(_, 3, _))),
        "{ops:?}"
    );
    assert!(has(&|o| matches!(o, FastOp::AddStoreReal(..))), "{ops:?}");
    assert!(
        has(&|o| matches!(o, FastOp::UnlessLessInt(_, 5, _))),
        "{ops:?}"
    );
    let micro = &program.micro;
    assert!(
        micro.iter().any(|m| matches!(m, Micro::AddInt(_, 1, _))),
        "{micro:?}"
    );
    assert!(
        micro.iter().any(|m| matches!(m, Micro::MulInt(_, 2, _))),
        "{micro:?}"
    );
    assert!(
        micro
            .iter()
            .any(|m| matches!(m, Micro::SubReal(_, x, _) if *x == 0.5))
    );
    assert!(
        micro
            .iter()
            .any(|m| matches!(m, Micro::MulReal(_, x, _) if *x == 1.5))
    );
    assert!(
        micro
            .iter()
            .any(|m| matches!(m, Micro::JumpGreaterEqInt(j) if j.k == 4))
    );
}

/// Statements of several operators run as chains.
#[test]
fn chains_take_statements_of_several_operators() {
    for source in [
        "A%=(A%+I*3) AND 1023",
        "DIM M%[8]:M%[I AND 7]=(M%[(I*3) AND 7]+I) AND 255",
        "IF X*X+Y*Y<R*R THEN ?1",
        "Q=(X+1)*(X+2)-X*3",
    ] {
        let program = compiled(source, true);
        let chains = (program.fast.iter())
            .filter(|f| matches!(f.op, FastOp::Chain { .. }))
            .count();
        assert!(chains > 0, "{source}: {:?}", program.fast);
    }
    let program = compiled(LOOP, true);
    assert!(
        program
            .fast
            .iter()
            .any(|f| matches!(f.op, FastOp::Chain { .. }))
    );
}

#[test]
fn most_loops_are_fused() {
    let program = compiled("FOR I=1 TO 9:A=A+I*2:NEXT", true);
    assert_eq!(program.fast.len(), program.code.len());
    assert!(program.fast.iter().any(|f| f.len > 3), "{:?}", program.fast);
}

/// Budgets that end in the middle of what would be fused, for each thing that
/// can stop a program.
const BUDGETS: [u32; 5] = [1, 4, 9, 23, 41];

const LOOP: &str = "\
A=0:DIM R%[8],K%[8]
FOR I=1 TO 6
 A=A+I*2
 B=A-1:C=B+1
 IF A>9 THEN D=1
 ON I MOD 3 GOSUB @S,@T
 CASE I
 WHEN 2:E=1
 WHEN 4:E=2
 ENDCASE
 K%[I]=I+1:R%[K%[I]]=R%[K%[I]]+1
 G=G+SQ(I):PQ I OUT H
 X=(A+I)*(A-I) AND 1023:R%[(I+1) AND 7]=(R%[I AND 7]+X) AND 255
 IF X*2>A+I THEN Y=Y+X*2
NEXT
?A
END
@S:F=F+1:RETURN
@T
RETURN
DEF SQ(N)
 RETURN N*N
END
DEF PQ N OUT M
 M=SQ(N)+1
END
";

/// Each line a run stops at, given that it first ran `budget` instructions
/// fused, and how `stop` asks for the stops.
fn stops(fused: bool, budget: u32, stop: impl Fn(&mut Vm)) -> Vec<(String, u64)> {
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled(LOOP, fused)).in_slot(0);
    vm.run(&mut host, budget).unwrap();
    let mut seen = Vec::new();
    for _ in 0..1000 {
        stop(&mut vm);
        let status = vm.run(&mut host, 1_000_000).unwrap();
        seen.push((format!("{status:?} line {}", vm.line()), vm.executed()));
        if status == Status::Ended {
            return seen;
        }
    }
    panic!("did not end");
}

#[test]
fn debugger_steps_stop_as_without_fusing() {
    for step in [Step::Into, Step::Over, Step::Out] {
        for budget in BUDGETS {
            let ask = |vm: &mut Vm| vm.step_by(step);
            assert_eq!(stops(true, budget, ask), stops(false, budget, ask));
        }
    }
}

#[test]
fn trace_stops_as_without_fusing() {
    for budget in BUDGETS {
        let ask = |vm: &mut Vm| vm.trace();
        let fused = stops(true, budget, ask);
        assert!(fused.len() > 6, "{fused:?}");
        assert_eq!(fused, stops(false, budget, ask));
    }
}

#[test]
fn breakpoints_stop_as_without_fusing() {
    for budget in BUDGETS {
        let set = |vm: &mut Vm| vm.set_breakpoints(0, &[(4, None)]);
        let fused = stops(true, budget, set);
        // The line in the loop, once each time round from wherever it began.
        assert!(fused.len() >= 5, "{fused:?}");
        assert_eq!(fused, stops(false, budget, set));
    }
}

#[test]
fn conditional_breakpoints_and_watches_stop_as_without_fusing() {
    let condition = Rc::new(Watch::new("A>10").unwrap());
    let watched = Rc::new(Watch::new("B").unwrap());
    for budget in BUDGETS {
        let set = |vm: &mut Vm| vm.set_breakpoints(0, &[(4, Some(condition.clone()))]);
        assert_eq!(stops(true, budget, set), stops(false, budget, set));
        let run = |fused: bool| {
            let mut host = TextHost::default();
            let mut vm = Vm::new(compiled(LOOP, fused)).in_slot(0);
            vm.run(&mut host, budget).unwrap();
            vm.set_changes(std::slice::from_ref(&watched));
            let mut seen = Vec::new();
            loop {
                let status = vm.run(&mut host, 1_000_000).unwrap();
                seen.push((format!("{status:?}"), vm.line(), vm.executed()));
                if status == Status::Ended {
                    return seen;
                }
            }
        };
        let fused = run(true);
        assert!(fused.len() >= 5, "{fused:?}");
        assert_eq!(fused, run(false));
    }
}

/// The + button comes between slices: wherever a slice ends in fused code, it
/// finds the program where it would have been.
#[test]
fn on_break_jumps_as_without_fusing() {
    // The second's statements run as chains, the last two as one.
    let sources = [
        "ON BREAK GOTO @B\nA=0\nWHILE 1\n A=A+1:C=A*2\nWEND\n@B\n?A\n?\"OUT\"",
        "ON BREAK GOTO @B\nA=0\nWHILE 1\n A=(A+1)*3 AND 255\n C=(A-1)*(A+1):D=C*2-A\nWEND\n@B\n?A;C;D",
    ];
    let budgets = [1, 2, 3, 5, 9, 17, 40, 100, 333];
    for (source, budget) in sources.iter().flat_map(|&s| budgets.map(|b| (s, b))) {
        let run = |fused: bool| {
            let mut host = TextHost::default();
            let mut vm = Vm::new(compiled(source, fused));
            let first = vm.run(&mut host, budget).unwrap();
            let taken = vm.take_break(&mut host);
            let line = vm.line();
            let status = vm.run(&mut host, 10_000).unwrap();
            (first, taken, line, status, vm.executed(), host.output)
        };
        assert_eq!(run(true), run(false), "{source} after {budget}");
    }
}

/// Without ON BREAK the host stops the program between slices, and the place
/// it stopped at goes on as if nothing had happened.
#[test]
fn a_stopped_slice_goes_on_as_without_fusing() {
    let source = "A=0\nFOR I=0 TO 50\n A=A+I:B=A*2\n STOP\nNEXT\n?A";
    let run = |fused: bool| {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, fused)).in_slot(0);
        let mut seen = Vec::new();
        loop {
            let status = vm.run(&mut host, 1_000_000).unwrap();
            seen.push((status, vm.line(), vm.executed()));
            if status == Status::Ended {
                return (seen, host.output);
            }
        }
    };
    assert_eq!(run(true), run(false));
}

/// A statement that fails names its own line and statement, whichever
/// instruction of a fused group it was.
#[test]
fn errors_name_their_statement() {
    let at = |source: &str| {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        let e = vm.run(&mut host, 1_000_000).unwrap_err();
        (e.kind.code(), e.line, e.col)
    };
    let plain = |source: &str| {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, false));
        let e = vm.run(&mut host, 1_000_000).unwrap_err();
        (e.kind.code(), e.line, e.col)
    };
    for source in [
        "A=1:B=0\nD=1:C=A/B:E=2",
        "DIM X[2]\nI=5\nD=1:E=X[I]:F=2",
        "DIM X[2]\nI=5\nD=1:X[I]=1:F=2",
        "S$=\"A\"\nFOR I=0 TO 1\nA=1:A=A+S$\nNEXT",
        "S$=\"A\"\nA=1\nFOR I=0 TO S$\nA=2\nNEXT",
        "S$=\"A\"\nT=1\nFOR I=0 TO T:T=S$\nNEXT",
        "DEF F(A)\nB=1:RETURN A/0\nEND\nC=2:D=F(1)",
        "A=1:B=0\nD=1:C=(A+1)/(B*2):E=2",
        "DIM X[2]\nI=5\nD=1:X[I-1]=X[0]*2+1:F=2",
        "A=1:B$=\"S\"\nD=A*2+1:C=(A+1)*2+B$:E=2",
        "DEF F(A)\nB=A*2+1:C=B*B\nRETURN (A+B)/(C-C)\nEND\nC=2:D=F(1)",
    ] {
        let fused = at(source);
        assert_eq!(fused, plain(source), "{source}");
        assert!(fused.1 > 0, "{source}: {fused:?}");
    }
}

/// What `source` prints and the code of the error it stops on, or 0.
fn outcome(source: &str, fused: bool) -> (String, i32) {
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled(source, fused));
    let code = match vm.run(&mut host, 1_000_000) {
        Ok(status) => {
            assert_eq!(status, Status::Ended, "{source}");
            0
        }
        Err(e) => e.kind.code(),
    };
    (host.output, code)
}

/// Runs `statement` after `setup`, on its own and in a FOR loop, fused and
/// in `step`, and checks what it leaves in Y or the error it stops on.
fn check_all(setup: &str, statement: &str, printed: &str, code: i32) {
    for source in [
        format!("{setup}\n{statement}\n?Y"),
        format!("{setup}\nFOR I=0 TO 1\n{statement}\nNEXT\n?Y"),
    ] {
        for fused in [true, false] {
            let (out, c) = outcome(&source, fused);
            assert_eq!(c, code, "{source} fused {fused}");
            if code == 0 {
                assert_eq!(out, format!("{printed}\n"), "{source} fused {fused}");
            }
        }
        for budget in [1, 2, 3, 5, 7] {
            assert_eq!(
                run(&source, budget, true),
                run(&source, budget, false),
                "{source}"
            );
        }
    }
}

/// Unary minus of -2147483648 wraps and stays an int (TEST65).
#[test]
fn negating_the_minimum_wraps() {
    check_all("A=&H80000000", "Y=-A", "-2147483648", 0);
    check_all("A=&H80000000", "Y=TYPEOF(-A)", "1", 0);
    check_all("A=&H7FFFFFFF", "Y=-A", "-2147483647", 0);
    // Binary minus still gives a real.
    check_all("A=&H80000000:Z=0", "Y=Z-A", "2147483648", 0);
    // Worked out when it compiles, the same way.
    assert_eq!(outcome("?-&H80000000", true), ("-2147483648\n".into(), 0));
}

/// An int array takes a real only in -2147483648..2147483647, before the
/// fraction is dropped (TEST65).
#[test]
fn int_array_stores_check_the_real() {
    for (value, printed, code) in [
        ("-3.9", "-3", 0),
        ("3.9", "3", 0),
        ("-0.5", "0", 0),
        ("2147483647.0", "2147483647", 0),
        ("2147483647.5", "", 9),
        ("2147483647.9", "", 9),
        ("-2147483648.5", "", 9),
        ("2147483648", "", 9),
        ("-2147483649", "", 9),
        ("4294967295", "", 9),
    ] {
        let setup = format!("DIM M%[2],N%[2,2]:J=1:X={value}");
        check_all(&setup, "M%[0]=X:Y=M%[0]", printed, code);
        check_all(&setup, "M%[J]=X:Y=M%[J]", printed, code);
        check_all(&setup, "N%[J,J]=X:Y=N%[J,J]", printed, code);
        // A % variable keeps the real.
        check_all(&setup, "Y%=X:Y=Y%==X", "1", 0);
    }
}

/// Bit operators saturate a real outside the 32-bit range, with no error
/// (TEST65). In range it is truncated toward zero.
#[test]
fn bit_operators_saturate_reals() {
    for (value, op, rhs, printed) in [
        ("-1.5", "AND", "-1", "-1"),
        ("3.9", "OR", "0", "3"),
        ("-3.9", "XOR", "0", "-3"),
        ("-2.5", ">>", "1", "-1"),
        ("-2.5", ">>>", "28", "15"),
        ("-2.5", "<<", "1", "-4"),
        ("2147483647.5", "AND", "-1", "2147483647"),
        ("-2147483648.5", "AND", "-1", "-2147483648"),
        ("2147483648", "AND", "-1", "2147483647"),
        ("-2147483649", "OR", "0", "-2147483648"),
        ("4294967295", "XOR", "0", "2147483647"),
        ("2147483648", "<<", "1", "-2"),
        ("2147483648", ">>", "1", "1073741823"),
        ("2147483648", ">>>", "1", "1073741823"),
        ("4294967295", ">>>", "0", "2147483647"),
        ("4294967296", "AND", "1", "1"),
        ("2147483648", ">>>", "0", "2147483647"),
        ("2147483648", "<<+", "1", "-2"),
        ("2147483648", ">>+", "1", "-1073741825"),
    ] {
        let setup = format!("X={value}:Z={rhs}");
        check_all(&setup, &format!("Y=X {op} {rhs}"), printed, 0);
        check_all(&setup, &format!("Y=X {op} Z"), printed, 0);
    }
    check_all("X=-2.5", "Y=NOT X", "1", 0);
    check_all("X=2147483648", "Y=NOT X", "-2147483648", 0);
    check_all("X=3.9:A=1", "Y=A<<X", "8", 0);
    check_all("X=2147483648", "Y=TYPEOF(X AND -1)", "1", 0);
}

/// Infinity and NaN made at run time, as TEST66 made them.
const INF_NAN: &str = "A=1E308:B=10:F=A*B:N=F-F:Z=-1:O=0:P=1:S=7";

/// A NaN operand of a bit operator is 0, with no error, and an infinity
/// saturates (TEST66).
#[test]
fn bit_operators_take_nan_as_zero() {
    for (statement, printed) in [
        ("Y=F AND Z", "2147483647"),
        ("Y=-F OR O", "-2147483648"),
        ("Y=NOT F", "-2147483648"),
        ("Y=F>>P", "1073741823"),
        ("Y=F<<+P", "-2"),
        ("Y=P<<F", "-2147483648"),
        ("Y=N AND Z", "0"),
        ("Y=Z AND N", "0"),
        ("Y=N OR O", "0"),
        ("Y=N XOR O", "0"),
        ("Y=NOT N", "-1"),
        ("Y=N<<P", "0"),
        ("Y=N>>>O", "0"),
        ("Y=N>>+P", "0"),
        ("Y=P<<N", "1"),
        ("Y=N AND -1", "0"),
        ("Y=N<<1", "0"),
        ("Y=TYPEOF(N AND Z)", "1"),
    ] {
        check_all(INF_NAN, statement, printed, 0);
    }
    // The right operand saturates too.
    check_all("P=1:X=2147483648", "Y=P<<X", "-2147483648", 0);
    check_all("Z=-1:X=2147483648", "Y=Z AND X", "2147483647", 0);
}

/// A NaN stored into an int array is 0 (TEST66).
#[test]
fn int_array_stores_take_nan_as_zero() {
    let setup = format!("{INF_NAN}:DIM M%[2],K%[2,2]:J=1");
    check_all(&setup, "M%[0]=N:Y=M%[0]", "0", 0);
    check_all(&setup, "M%[J]=N:Y=M%[J]", "0", 0);
    check_all(&setup, "K%[J,J]=N:Y=K%[J,J]", "0", 0);
    check_all(&setup, "M%[0]=F:Y=M%[0]", "", 9);
}

/// DIV and MOD check a real operand as an int array store does: outside
/// -2147483648..2147483647 before truncation is Overflow, and a NaN is 0
/// (TEST66).
#[test]
fn div_and_mod_check_the_real() {
    for (setup, statement, printed, code) in [
        ("X=2147483647.5:D=1", "Y=X DIV D", "", 9),
        ("X=2147483647.5:D=1", "Y=X DIV 1", "", 9),
        ("X=-2147483648.5:D=10", "Y=X MOD D", "", 9),
        ("X=-2147483648.5:D=10", "Y=X MOD 10", "", 9),
        ("X=2147483648:D=2", "Y=X DIV D", "", 9),
        ("X=4294967296:D=7", "Y=X MOD D", "", 9),
        ("X=-2147483649:D=1", "Y=X DIV D", "", 9),
        ("X=7:D=2147483648", "Y=X DIV D", "", 9),
        ("X=7:D=-2147483649", "Y=X MOD D", "", 9),
        ("X=7.9:D=2.1", "Y=X DIV D", "3", 0),
        ("X=-7.9:D=2", "Y=X MOD D", "-1", 0),
        ("X=2147483647.0:D=2", "Y=X DIV D", "1073741823", 0),
        ("X=-2147483648.0:D=10", "Y=X MOD D", "-8", 0),
    ] {
        check_all(setup, statement, printed, code);
    }
    check_all(INF_NAN, "Y=F DIV P", "", 9);
    check_all(INF_NAN, "Y=N MOD S", "0", 0);
    check_all(INF_NAN, "Y=TYPEOF(N MOD S)", "1", 0);
}

/// PUSH and UNSHIFT check a real as an int array store does (TEST66).
#[test]
fn push_checks_the_real() {
    for (value, printed, code) in [
        ("-3.9", "-3", 0),
        ("2147483647.0", "2147483647", 0),
        ("2147483647.5", "", 9),
        ("-2147483648.5", "", 9),
        ("2147483648", "", 9),
    ] {
        let setup = format!("DIM M%[0]:X={value}");
        check_all(&setup, "PUSH M%,X:Y=M%[LAST(M%)]", printed, code);
        check_all(&setup, "UNSHIFT M%,X:Y=M%[0]", printed, code);
    }
    check_all(&format!("{INF_NAN}:DIM M%[0]"), "PUSH M%,N:Y=M%[0]", "0", 0);
}

/// The error a program stops on, fused or not, which must be the same.
fn error_of(source: &str) -> sb_core::error::Error {
    let [fused, plain] = [true, false].map(|fused| {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, fused));
        vm.run(&mut host, 1_000_000).unwrap_err()
    });
    assert_eq!(fused, plain, "{source}");
    fused
}

/// The builtins SmileBASIC 4 is seen to name in an error, as it names them,
/// and the argument ENVSTAT gives (TEST66). Operators name nothing.
#[test]
fn builtin_errors_name_the_command() {
    for (source, message, arg) in [
        (
            "DIM M%[3]:X=2147483647.5\nFILL M%,X",
            "Overflow in 2(FILL:2)",
            Some(1),
        ),
        (
            "DIM M%[0]:X=2147483647.5\nPUSH M%,X",
            "Overflow in 2(PUSH)",
            None,
        ),
        (
            "DIM M%[0]:X=2147483648\nPUSH M%,X",
            "Overflow in 2(PUSH)",
            None,
        ),
        (
            "DIM M%[2],R[2]:R[0]=2147483647.5\nCOPY M%,R",
            "Overflow in 2(COPY)",
            None,
        ),
        ("X=2147483648\nS$=HEX$(X)", "Overflow in 2(HEX$:1)", Some(0)),
        (
            "X=-2147483649\nS$=HEX$(X)",
            "Overflow in 2(HEX$:1)",
            Some(0),
        ),
        ("X=4294967361\nS$=CHR$(X)", "Overflow in 2(CHR$:1)", Some(0)),
        (
            "X=2147483648\nS$=MID$(\"ABC\",X,1)",
            "Overflow in 2(MID$:2)",
            Some(1),
        ),
        (
            "X=2147483648\nS$=LEFT$(\"ABC\",X)",
            "Overflow in 2(LEFT$)",
            None,
        ),
        // TEST67: every builtin names itself.
        (
            "S$=\"ABC\":X=-1\nS$=MID$(S$,X,1)",
            "Out of range in 2(MID$:2)",
            Some(1),
        ),
        (
            "DIM M%[3]:X=5\nFILL M%,0,X",
            "Out of range in 2(FILL:3)",
            Some(2),
        ),
        (
            "DIM M%[0]:X=2147483648\nUNSHIFT M%,X",
            "Overflow in 2(UNSHIFT)",
            None,
        ),
        ("X=0\nY=LOG(X)", "Out of range in 2(LOG:1)", Some(0)),
        ("X=2\nY=ASIN(X)", "Out of range in 2(ASIN:1)", Some(0)),
        // SQR names nothing.
        ("X=-1\nY=SQR(X)", "Out of range in 2", None),
        (
            "S$=\"ABC\":X=-1\nS$=LEFT$(S$,X)",
            "Out of range in 2(LEFT$:2)",
            Some(1),
        ),
        (
            "S$=\"ABC\":X=-1\nS$=RIGHT$(S$,X)",
            "Out of range in 2(RIGHT$:2)",
            Some(1),
        ),
        (
            "DIM A[3],B[3]:X=5\nCOPY A,0,B,2,X",
            "Subscript out of range in 2(COPY) (count)",
            None,
        ),
        ("X=2147483647.5:D=1\nY=X DIV D", "Overflow in 2", None),
        ("X=0:D=1\nY=D MOD X", "Divide by zero in 2", None),
        ("DIM M%[1]:X=2147483648\nM%[0]=X", "Overflow in 2", None),
    ] {
        let e = error_of(source);
        assert_eq!((e.to_string().as_str(), e.arg), (message, arg), "{source}");
    }
}

mod common;
use common::CmdHost;

/// [`run`] on a host with commands of its own, which it compiles with.
fn run_on_host(source: &str, budget: u32, fused: bool) -> (Vec<String>, String) {
    let mut host = CmdHost::default();
    let mut program = sb_core::compile(source, &host).unwrap_or_else(|e| panic!("{source}: {e}"));
    if !fused {
        program.fast.clear();
    }
    let mut vm = Vm::new(program).in_slot(0);
    let mut seen = Vec::new();
    for _ in 0..200_000 {
        let result = vm.run(&mut host, budget);
        let shown = match &result {
            Ok(status) => format!("{status:?}"),
            Err(e) => format!("{e} {:?} {} {} {:?}", e.kind, e.line, e.col, e.detail),
        };
        let (executed, line, col) = (vm.executed(), vm.line(), vm.col());
        seen.push(format!("{shown} executed {executed} line {line} col {col}"));
        match result {
            Ok(Status::Ended) | Err(_) => {
                seen.push(format!("count {} held {}", host.count, host.held));
                return (seen, host.text.output);
            }
            Ok(_) => {}
        }
    }
    panic!("{source}: did not end");
}

const HOST_PROGRAMS: &[&str] = &[
    "FOR I=0 TO 5:PUT I,I*2:A=GET():?A:NEXT",
    "DEF F N\nPUT N:GET N\nRETURN\nEND\nFOR I=0 TO 3:F I:?GET():NEXT",
    "FOR I=0 TO 3:PAIR I OUT B,C:?B;C:GET OUT D:?D:NEXT",
    "?GET()+GET()*2:A=1+GET()*GET():?A:B=MAX(GET(),3)+ABS(GET()):?B",
    "DEF R(N)\nPUT N\nIF N<1 THEN RETURN GET()\nRETURN R(N-1)+GET()\nEND\n?R(4)",
    "DEF F(N)\nPUT N\nRETURN N\nEND\nFOR I=2 TO -2 STEP -1:A=F(I):?A:NEXT",
    "FOR I=0 TO 3:HOLD:?I:NEXT:HOLD:HOLD:?\"END\"",
    "FOR I=0 TO 2:S$=NOTE$():?S$;LEN(S$):NEXT",
    "FOR I=3 TO -1 STEP -1:PUT I:NEXT",
    "A$=\"X\"\n?1:PUT 1,A$:?2",
    "A=1\n?1:B=BAD():?2",
    "A=1\nFOR I=0 TO 2\nPUT I OUT X\nNEXT",
    "DEF G A OUT B\nPUT A:B=GET()\nEND\nFOR I=0 TO 3:G I OUT C:?C:NEXT:G -1 OUT C",
    "DEF H(A)\nPAIR A OUT B,C\nRETURN B*C\nEND\nFOR I=0 TO 3:?H(I)+H(I+1):NEXT",
];

/// Host calls made fused leave the same stops, output, errors and counts as
/// made by `step`, in slices that end anywhere.
#[test]
fn host_calls_run_the_same_fused_or_not() {
    for program in HOST_PROGRAMS {
        for budget in [1, 2, 3, 5, 7, 13, 100, 1_000_000] {
            let fused = run_on_host(program, budget, true);
            let plain = run_on_host(program, budget, false);
            assert_eq!(fused, plain, "{program} in slices of {budget}");
        }
    }
    // GOSUBs that fill the stack's room for values, which a command given
    // arguments then finds full, in the longer slices that take.
    for program in [
        "?1:GOSUB @S\n@S:PUT 1:GOSUB @S",
        "?1:GOSUB @S\n@S:A=GET():GOSUB @S",
    ] {
        for budget in [5, 7, 1_000_000] {
            let fused = run_on_host(program, budget, true);
            assert!(
                fused.0.iter().any(|s| s.contains("StackOverflow")),
                "{fused:?}"
            );
            assert_eq!(
                fused,
                run_on_host(program, budget, false),
                "{program} in {budget}"
            );
        }
    }
}

/// Host calls are fused entries.
#[test]
fn host_calls_are_fused() {
    let host = CmdHost::default();
    let program = sb_core::compile("FOR I=0 TO 3:PUT I:A=GET():NEXT", &host).unwrap();
    let calls = (program.fast.iter())
        .filter(|f| matches!(f.op, FastOp::Host { .. }))
        .count();
    assert_eq!(calls, 2, "{:?}", program.fast);
}

/// A debugger sees each DEF call's own variables, and changes one of them,
/// with calls under way inside calls.
#[test]
fn each_call_keeps_its_own_variables() {
    let source = "\
DEF A X
 VAR Y=X*2
 B Y+1
 ?X;Y
END
DEF B Z
 VAR W$=\"S\"
 STOP
 ?Z;W$
END
FOR I=0 TO 1:A 5+I:NEXT
";
    let run = |fused: bool| {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, fused)).in_slot(0);
        let mut seen = Vec::new();
        loop {
            let status = vm.run(&mut host, 1_000_000).unwrap();
            if let Status::Stop { .. } = status {
                let frames: Vec<_> = vm.calls().iter().filter_map(|c| c.frame).collect();
                seen.push(format!("{frames:?}"));
                for &f in &frames {
                    seen.push(format!("{:?}", vm.frame_locals(f)));
                }
                assert!(vm.set_local(0, "Y", sb_core::value::Value::Int(3)));
                assert!(!vm.set_local(1, "Y", sb_core::value::Value::Int(3)));
            }
            if status == Status::Ended {
                return (seen, host.output);
            }
        }
    };
    let fused = run(true);
    assert_eq!(fused.1, "11S\n53\n13S\n63\n");
    assert!(fused.0[1].contains("Int(10)"), "{fused:?}");
    assert!(fused.0[2].contains("Int(11)"), "{fused:?}");
    assert_eq!(fused, run(false));
}

/// Runs `program` as given, fused code and all, in slices of `budget`.
fn run_program(program: Compiled, budget: u32) -> (Vec<String>, String) {
    let mut host = TextHost::default();
    let mut vm = Vm::new(program);
    let mut seen = Vec::new();
    for _ in 0..100_000 {
        let result = vm.run(&mut host, budget);
        seen.push(format!("{result:?} {} {}", vm.executed(), vm.line()));
        if matches!(result, Ok(Status::Ended) | Err(_)) {
            return (seen, host.output);
        }
    }
    panic!("did not end");
}

const BIG_AND_SMALL: &str = "\
DEF BIG
 VAR A,B,C,D,E,F,G,H,I,J
 J=J+1:?J
END
DEF SMALL
 VAR K
 K=K+1:?K
END
BIG:SMALL:BIG:SMALL
";

/// An entry that names more of a DEF's variables than the running DEF has,
/// wherever it came from: here one of BIG's entries, put in SMALL's place.
/// With `unchecked-vars`, which reads places without checking each, the VM
/// must leave it to `step`, and the program prints what it prints unfused;
/// in a debug build, a read past the DEF's variables would fail its
/// assertion. Without the feature, the read's own bounds check stops it.
#[test]
fn entries_naming_more_variables_than_the_def_has_run_from_code() {
    let mut program = compiled(BIG_AND_SMALL, true);
    let adds: Vec<usize> = (0..program.fast.len())
        .filter(|&i| matches!(program.fast[i].op, FastOp::AddStoreInt(..)))
        .collect();
    assert_eq!(adds.len(), 2, "{:?}", program.fast);
    let (big, small) = (adds[0], adds[1]);
    assert_eq!((program.fast[big].need, program.fast[small].need), (10, 1));
    program.fast[small].op = program.fast[big].op;
    // A need set by hand does not count: the VM works it out again.
    program.fast[small].need = 0;
    let expected = run(BIG_AND_SMALL, 1_000_000, false);
    assert_eq!(expected.1, "1\n1\n1\n1\n");
    let mut stopped = 0;
    for budget in [1, 3, 1_000_000] {
        let tampered = program.clone();
        if cfg!(feature = "unchecked-vars") {
            assert_eq!(run_program(tampered, budget).1, expected.1);
            continue;
        }
        let run = std::panic::catch_unwind(AssertUnwindSafe(|| run_program(tampered, budget)));
        match run {
            Ok(run) => assert_eq!(run.1, expected.1),
            Err(panic) => {
                let text = (panic.downcast_ref::<String>().cloned())
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                assert!(text.contains("index out of bounds"), "{text}");
                stopped += 1;
            }
        }
    }
    assert!(cfg!(feature = "unchecked-vars") || stopped > 0);
}

/// Each entry's need is the locals it names, by a count of its own.
#[test]
fn each_entry_needs_the_locals_it_names() {
    let more = [BIG_AND_SMALL, CONSTANTS_IN_DEF, DEF_OUT, DEF_RECURSIVE];
    for source in PROGRAMS.iter().chain(&more) {
        common::check_needs(&compiled(source, true), source);
    }
}

/// A global place beyond the program's globals and constants makes the VM
/// run the program without its fused code.
#[test]
fn entries_naming_places_beyond_the_globals_are_not_run() {
    let many = (0..300).map(|i| format!("+{i}")).collect::<String>();
    let donor = compiled(&format!("A=0{many}:B=A+299:?B"), true);
    let far = (donor.fast.iter())
        .find_map(|f| match f.op {
            FastOp::Push(v) if v.index() > 200 => Some(f.op),
            FastOp::Push2(_, v) if v.index() > 200 => Some(f.op),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{:?}", donor.fast));
    let source = "A=1:B=A+2:?B";
    let mut program = compiled(source, true);
    let at = (program.fast.iter())
        .position(|f| matches!(f.op, FastOp::AddStoreInt(..)))
        .unwrap_or_else(|| panic!("{:?}", program.fast));
    program.fast[at].op = far;
    let vm = Vm::new(program.clone());
    assert!(vm.program().fast.is_empty());
    assert_eq!(run_program(program, 1).1, run(source, 1, false).1);
}
