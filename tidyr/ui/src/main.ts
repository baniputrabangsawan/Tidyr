import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

const target = document.getElementById("app");
const app = target ? mount(App, { target }) : undefined;

export default app;
