import { ToastProvider } from "./components";
import { AppShell } from "./shell/AppShell";

export function App() {
  return (
    <ToastProvider>
      <AppShell />
    </ToastProvider>
  );
}
