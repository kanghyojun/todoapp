import type { InvokeFn, ListenFn } from "../client";
import type {
  GmailAccount,
  MailBody,
  MailFilter,
  MailListItem,
} from "./domain";

export interface GmailClient {
  accounts(): Promise<GmailAccount[]>;
  list(filter: MailFilter): Promise<MailListItem[]>;
  getBody(accountId: string, gmailId: string): Promise<MailBody>;
  archive(accountId: string, gmailId: string): Promise<void>;
  setRead(accountId: string, gmailId: string, read: boolean): Promise<void>;
  sync(): Promise<void>;
  addAccount(): Promise<GmailAccount>;
  removeAccount(accountId: string): Promise<void>;
  setCredentials(clientId: string, clientSecret: string): Promise<void>;
  subscribe(onChange: () => void): () => void;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNullableString(value: unknown): value is string | null {
  return typeof value === "string" || value === null;
}

export class GmailClientError extends Error {
  constructor(
    message: string,
    readonly code: string,
  ) {
    super(message);
    this.name = "GmailClientError";
  }
}

function gmailError(value: unknown): Error {
  if (
    isRecord(value) &&
    typeof value.code === "string" &&
    typeof value.message === "string"
  ) {
    return new GmailClientError(value.message, value.code);
  }
  if (value instanceof Error) {
    return value;
  }
  return new GmailClientError(
    typeof value === "string" ? value : "Gmail 명령이 실패했습니다.",
    "ipc_error",
  );
}

export function decodeAccount(value: unknown): GmailAccount {
  if (
    !isRecord(value) ||
    typeof value.id !== "string" ||
    typeof value.email !== "string" ||
    typeof value.color !== "string" ||
    typeof value.sync_state !== "string" ||
    !isNullableString(value.last_error)
  ) {
    throw new GmailClientError(
      "server returned an invalid account",
      "invalid_response",
    );
  }
  return {
    id: value.id,
    email: value.email,
    color: value.color,
    sync_state: value.sync_state,
    last_error: value.last_error,
  };
}

export function decodeAccounts(value: unknown): GmailAccount[] {
  if (!Array.isArray(value)) {
    throw new GmailClientError(
      "server returned an invalid account list",
      "invalid_response",
    );
  }
  return value.map(decodeAccount);
}

export function decodeMailListItem(value: unknown): MailListItem {
  if (
    !isRecord(value) ||
    typeof value.account_id !== "string" ||
    typeof value.account_email !== "string" ||
    typeof value.account_color !== "string" ||
    typeof value.gmail_id !== "string" ||
    typeof value.thread_id !== "string" ||
    typeof value.from_name !== "string" ||
    typeof value.from_email !== "string" ||
    typeof value.subject !== "string" ||
    typeof value.snippet !== "string" ||
    typeof value.internal_date !== "number" ||
    typeof value.in_inbox !== "boolean" ||
    typeof value.is_unread !== "boolean" ||
    typeof value.has_todo !== "boolean"
  ) {
    throw new GmailClientError(
      "server returned an invalid mail item",
      "invalid_response",
    );
  }
  return {
    account_id: value.account_id,
    account_email: value.account_email,
    account_color: value.account_color,
    gmail_id: value.gmail_id,
    thread_id: value.thread_id,
    from_name: value.from_name,
    from_email: value.from_email,
    subject: value.subject,
    snippet: value.snippet,
    internal_date: value.internal_date,
    in_inbox: value.in_inbox,
    is_unread: value.is_unread,
    has_todo: value.has_todo,
  };
}

export function decodeMailList(value: unknown): MailListItem[] {
  if (!Array.isArray(value)) {
    throw new GmailClientError(
      "server returned an invalid mail list",
      "invalid_response",
    );
  }
  return value.map(decodeMailListItem);
}

export function decodeBody(value: unknown): MailBody {
  if (
    !isRecord(value) ||
    typeof value.gmail_id !== "string" ||
    !isNullableString(value.body_text) ||
    !isNullableString(value.body_html)
  ) {
    throw new GmailClientError(
      "server returned an invalid mail body",
      "invalid_response",
    );
  }
  return {
    gmail_id: value.gmail_id,
    body_text: value.body_text,
    body_html: value.body_html,
  };
}

export class TauriGmailClient implements GmailClient {
  constructor(
    private readonly invoke: InvokeFn,
    private readonly listen: ListenFn,
  ) {}

  async accounts(): Promise<GmailAccount[]> {
    return decodeAccounts(await this.request("gmail_accounts"));
  }

  async list(filter: MailFilter): Promise<MailListItem[]> {
    return decodeMailList(await this.request("gmail_list", { filter }));
  }

  async getBody(accountId: string, gmailId: string): Promise<MailBody> {
    return decodeBody(
      await this.request("gmail_get_body", { accountId, gmailId }),
    );
  }

  async archive(accountId: string, gmailId: string): Promise<void> {
    await this.request("gmail_archive", { accountId, gmailId });
  }

  async setRead(
    accountId: string,
    gmailId: string,
    read: boolean,
  ): Promise<void> {
    await this.request("gmail_set_read", { accountId, gmailId, read });
  }

  async sync(): Promise<void> {
    await this.request("gmail_sync");
  }

  async addAccount(): Promise<GmailAccount> {
    return decodeAccount(await this.request("gmail_add_account"));
  }

  async removeAccount(accountId: string): Promise<void> {
    await this.request("gmail_remove_account", { accountId });
  }

  async setCredentials(clientId: string, clientSecret: string): Promise<void> {
    await this.request("gmail_set_credentials", { clientId, clientSecret });
  }

  subscribe(onChange: () => void): () => void {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void this.listen("mail:changed", onChange)
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }

  private async request(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<unknown> {
    try {
      return await this.invoke(command, args);
    } catch (error) {
      throw gmailError(error);
    }
  }
}
