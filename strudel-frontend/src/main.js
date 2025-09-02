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
<button id="socket">socket</button>
`;

const click = (id, action) =>
	document.getElementById(id).addEventListener("click", action);
click("a", () => evaluate(`s('bd,jvbass(3,8)').jux(rev)`));
click("b", () => evaluate(`s("bd*2,hh(3,4),jvbass(5,8,1)").jux(rev)`));
click("c", () =>
	evaluate(`s("bd*2,hh(3,4),jvbass:[0 4](5,8,1)").jux(rev).stack(s("~ sd"))`),
);
click("stop", () => evaluate("hush()"));

click("socket", () => {
	let host = window.location.host;
	const socket = new WebSocket("ws://" + host + "/ws");

	socket.addEventListener("open", (msg) => console.log(msg));
	socket.addEventListener("message", socket_message_handler);
	socket.addEventListener("close", (msg) => console.log(msg));
});

function socket_message_handler(msg) {
	console.log(msg);
	let data = JSON.parse(msg.data);
	console.log(data);
}
