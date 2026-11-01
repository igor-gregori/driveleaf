import { Component, type ErrorInfo, type ReactNode } from "react";
import { t } from "@/i18n";

interface State {
  message: string | null;
}

/** Keeps a bug from leaving a blank window, and shows what went wrong. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { message: null };

  static getDerivedStateFromError(error: unknown): State {
    return { message: String(error) };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error(error, info.componentStack);
  }

  render() {
    if (this.state.message === null) {
      return this.props.children;
    }
    return (
      <div className="grid h-screen place-items-center bg-background p-8 text-center">
        <div className="max-w-md">
          <h1 className="mb-2 text-base font-semibold">{t("error.title")}</h1>
          <p className="mb-4 text-sm text-muted-foreground">{t("error.body")}</p>
          <pre className="mb-4 rounded-md bg-muted p-3 text-left text-xs break-all whitespace-pre-wrap">
            {this.state.message}
          </pre>
          <button
            type="button"
            className="text-sm underline"
            onClick={() => window.location.reload()}
          >
            {t("error.reload")}
          </button>
        </div>
      </div>
    );
  }
}
