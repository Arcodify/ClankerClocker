import { mount } from "svelte";
import App from "./App.svelte";
import TasksApp from "./components/tasks/TasksApp.svelte";

// The Tasks window loads the same bundle with ?window=tasks.
const isTasksWindow = new URLSearchParams(location.search).get("window") === "tasks";

const app = mount(isTasksWindow ? TasksApp : App, { target: document.getElementById("app")! });

export default app;
