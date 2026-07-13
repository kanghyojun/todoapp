export type MailFolder = "inbox" | "archive" | "all";

export interface GmailAccount {
  id: string;
  email: string;
  color: string;
  sync_state: string;
  last_error: string | null;
}

export interface MailListItem {
  account_id: string;
  account_email: string;
  account_color: string;
  gmail_id: string;
  thread_id: string;
  from_name: string;
  from_email: string;
  subject: string;
  snippet: string;
  internal_date: number;
  in_inbox: boolean;
  is_unread: boolean;
  has_todo: boolean;
}

export interface MailBody {
  gmail_id: string;
  body_text: string | null;
  body_html: string | null;
}

export interface MailFilter {
  folder: MailFolder;
  account_id?: string;
  q?: string;
  limit?: number;
}
