// The desktop window's own account (account.ts): the dialog that creates the first account or signs
// in, the note of a node whose first account is made elsewhere or that cannot be asked, and the
// header chip with the account's name and Sign out. Every word is the launcher's string table in
// the user's language. A labelled modal dialog: the first field has the focus, Enter sends, Escape
// steps back (or puts a note away), an error is announced and sends the focus to its field, and the
// password fields are emptied after every answer.
import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";

import {
  checkForm, chipText, dismissible, nodeName, refusalText, say,
  type AccountState, type AccountView, type FieldName, type FormFields, type FormMode, type SignInAnswer,
} from "./account";

export type Submit = (mode: FormMode, username: string, password: string) => Promise<SignInAnswer>;

/** What the dialog shows besides its fields: an error at a field, or the empty-password question. */
export interface FormStatus {
  mode: FormMode;
  error: string | null;
  field: FieldName | null;
  askEmpty: boolean;
  busy: boolean;
}

/**
 * One press of the dialog's button: the terminal's checks, then the server. Resolves with the next
 * status, whether the passwords are emptied, and whether the account is signed in.
 */
export async function pressSend(
  text: Record<string, string>, status: FormStatus, fields: FormFields, emptyConfirmed: boolean, submit: Submit,
): Promise<{ status: FormStatus; clear: boolean; done: boolean }> {
  const check = checkForm(status.mode, fields);
  if (check.kind === "error") return { status: { ...status, error: say(text, check.key), field: check.field, askEmpty: false, busy: false }, clear: false, done: false };
  if (check.kind === "askEmpty" && !emptyConfirmed) return { status: { ...status, error: null, field: null, askEmpty: true, busy: false }, clear: false, done: false };
  let answer: SignInAnswer;
  try {
    answer = await submit(status.mode, check.username, fields.password);
  } catch (error) {
    answer = { ok: false, code: null, message: error instanceof Error ? error.message : String(error) };
  }
  if (answer.ok) return { status: { ...status, error: null, field: null, askEmpty: false, busy: false }, clear: true, done: true };
  const field: FieldName = status.mode === "create" && answer.code === -32002 ? "username" : "password";
  return { status: { ...status, error: refusalText(text, answer), field, askEmpty: false, busy: false }, clear: true, done: false };
}

const firstMode = (view: AccountView): FormMode => (view.kind === "createFirst" ? "create" : "signIn");

export function AccountGate(props: {
  state: AccountState;
  view: AccountView;
  submit: Submit;
  onSignedIn: () => void;
  onRetry: () => void;
  onDismiss: () => void;
  /** For the test renderer: the status to draw first. */
  initial?: Partial<FormStatus>;
}) {
  const { state, view } = props;
  const text = state.text;
  const node = nodeName(state);
  const [status, setStatus] = useState<FormStatus>({ mode: firstMode(view), error: null, field: null, askEmpty: false, busy: false, ...props.initial });
  const [fields, setFields] = useState<FormFields>({ username: "", password: "", confirm: "" });
  const inputs = { username: useRef<HTMLInputElement>(null), password: useRef<HTMLInputElement>(null), confirm: useRef<HTMLInputElement>(null) };
  const askButton = useRef<HTMLButtonElement>(null);
  const note = useRef<HTMLButtonElement>(null);
  // The focus follows the dialog: to the field an error names, to the question, else the first field.
  useEffect(() => {
    if (status.busy) return;
    if (status.askEmpty) askButton.current?.focus();
    else (inputs[status.field ?? "username"].current ?? note.current)?.focus();
  }, [status.askEmpty, status.field, status.mode, status.busy, status.error]);

  const close = dismissible(view) ? props.onDismiss : null;
  const keys = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    if (status.askEmpty) setStatus({ ...status, askEmpty: false, field: "password" });
    else if (view.kind === "signIn" && status.mode === "create") switchMode("signIn");
    else close?.();
  };
  const switchMode = (mode: FormMode) => {
    setFields({ username: fields.username, password: "", confirm: "" });
    setStatus({ mode, error: null, field: null, askEmpty: false, busy: false });
  };
  const send = async (emptyConfirmed: boolean) => {
    if (status.busy) return;
    setStatus({ ...status, busy: true, error: null });
    const next = await pressSend(text, status, fields, emptyConfirmed, props.submit);
    if (next.clear) setFields((current) => ({ ...current, password: "", confirm: "" }));
    setStatus(next.status);
    if (next.done) props.onSignedIn();
  };
  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    void send(false);
  };

  if (view.kind === "remoteSetup" || view.kind === "unreachable") {
    const body = view.kind === "remoteSetup" ? say(text, "FirstAccountOnNode", node) : `${say(text, "AccountsUnreachable")} ${view.error}`;
    return (
      <div className="dialog-backdrop" onKeyDown={keys}>
        <section className="dialog account-gate" role="dialog" aria-modal="true" aria-labelledby="account-title" aria-describedby="account-body">
          <header><h2 id="account-title">{say(text, "AccountLabel")}</h2></header>
          <p id="account-body" className="dialog-note" role={view.kind === "unreachable" ? "alert" : undefined}>{body}</p>
          <div className="buttons dialog-buttons">
            <button ref={note} className="primary" onClick={props.onRetry}>{say(text, "RetryAction")}</button>
            <button onClick={props.onDismiss}>{say(text, "NotNowAction")}</button>
          </div>
        </section>
      </div>
    );
  }

  const create = status.mode === "create";
  const first = view.kind === "createFirst";
  const title = first ? say(text, "CreateFirstTitle", node) : create ? say(text, "CreateTitle", node) : say(text, "SignInTitle", node);
  const hint = first ? say(text, "CreateFirstHint", node) : create ? say(text, "CreateHint") : say(text, "SignInHint");
  const invalid = (field: FieldName) => (status.error && status.field === field ? true : undefined);
  const describedBy = (field: FieldName) => (status.error && status.field === field ? "account-error" : undefined);
  const field = (name: FieldName, label: string, type: "text" | "password", autoComplete: string) => (
    <label className="field" htmlFor={`account-${name}`}>
      <span>{say(text, label)}</span>
      <input ref={inputs[name]} id={`account-${name}`} name={name} type={type} autoComplete={autoComplete} spellCheck={false} autoCapitalize="off"
        required={name === "username"} maxLength={name === "username" ? 32 : 1024} value={fields[name]} disabled={status.busy}
        aria-invalid={invalid(name)} aria-describedby={describedBy(name)}
        onChange={(event) => setFields({ ...fields, [name]: event.target.value })} />
    </label>
  );
  return (
    <div className="dialog-backdrop" onKeyDown={keys}>
      <section className="dialog account-gate" role="dialog" aria-modal="true" aria-labelledby="account-title" aria-describedby="account-hint" aria-busy={status.busy || undefined}>
        <header><h2 id="account-title">{title}</h2></header>
        <p id="account-hint" className="dialog-note">{hint}</p>
        {status.askEmpty ? (
          <div className="account-ask" role="alertdialog" aria-labelledby="account-ask">
            <p id="account-ask">{say(text, "EmptyPasswordAsk")}</p>
            <div className="buttons dialog-buttons">
              <button ref={askButton} className="primary" onClick={() => void send(true)}>{say(text, "ContinueAction")}</button>
              <button onClick={() => setStatus({ ...status, askEmpty: false, field: "password" })}>{say(text, "BackAction")}</button>
            </div>
          </div>
        ) : (
          <form onSubmit={onSubmit} noValidate>
            {field("username", "FieldUsername", "text", "username")}
            {field("password", "FieldPassword", "password", create ? "new-password" : "current-password")}
            {create ? field("confirm", "FieldConfirmPassword", "password", "new-password") : null}
            {status.error ? <p id="account-error" className="account-error" role="alert">{status.error}</p> : null}
            {status.busy ? <p className="dialog-note" aria-live="polite">{say(text, "SigningIn")}</p> : null}
            <div className="buttons dialog-buttons">
              <button type="submit" className="primary" disabled={status.busy}>{say(text, create ? "CreateAction" : "SignInAction")}</button>
              {view.kind === "signIn" ? (
                <button type="button" className="link-button" disabled={status.busy} onClick={() => switchMode(create ? "signIn" : "create")}>
                  {say(text, create ? "SignInInstead" : "CreateInstead")}
                </button>
              ) : null}
            </div>
          </form>
        )}
      </section>
    </div>
  );
}

/** The header's chip: the account's name and role, and Sign out. */
export function AccountChip(props: { state: AccountState; view: Extract<AccountView, { kind: "signedIn" }>; onSignOut: () => void; busy?: boolean }) {
  const text = props.state.text;
  const label = chipText(props.state, props.view);
  return (
    <span className="chip account-chip" role="group" aria-label={say(text, "AccountLabel")} title={say(text, "SignedInOnly", nodeName(props.state))}>
      <span className="account-name">{label}</span>
      <button className="link-button" disabled={props.busy} onClick={props.onSignOut}>{say(text, "SignOutAction")}</button>
    </span>
  );
}
