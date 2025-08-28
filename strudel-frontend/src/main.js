import { initStrudel } from "@strudel/web";
initStrudel({
  prebake: () => samples("github:tidalcycles/dirt-samples"),
});

const app = document.getElementById("app");
app.innerHTML = `
<button id="a">A</button>
<button id="b">B</button>
<button id="c">C</button>
<button id="stop">stop</button>
`;

const click = (id, action) =>
  document.getElementById(id).addEventListener("click", action);
click("a", () => evaluate(`s('bd,jvbass(3,8)').jux(rev)`));
click("b", () => evaluate(`s("bd*2,hh(3,4),jvbass(5,8,1)").jux(rev)`));
click("c", () =>
  evaluate(`s("bd*2,hh(3,4),jvbass:[0 4](5,8,1)").jux(rev).stack(s("~ sd"))`),
);
click("stop", () => evaluate("hush()"));
