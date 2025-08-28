const app = document.querySelector("#app");

let paragraph = document.createElement("p");
paragraph.textContent = "hello world";

let button = document.createElement("button");
button.appendChild(document.createTextNode("button"));
button.addEventListener("click", () => {
  console.log("hello");
});

app.appendChild(paragraph);
app.appendChild(button);
