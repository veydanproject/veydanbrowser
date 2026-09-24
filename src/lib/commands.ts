// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

/**
 * Global command registry behind the command palette. Static commands are
 * registered once; providers return commands derived from data (one per
 * SSH connection, smart view, ...) and run each time the palette lists them.
 */

export interface Command {
  /** Stable id, e.g. `notes.create` or `ssh.connect.{id}` */
  id: string;
  title: string;
  /** Extra words the search should match */
  keywords?: string;
  icon?: string;
  /** Section shown in the palette */
  group: string;
  /** Hidden when false */
  when?: () => boolean;
  run: () => void | Promise<void>;
}

export type CommandProvider = () => Command[];

class CommandRegistry {
  private static_ = new Map<string, Command>();
  private providers: CommandProvider[] = [];

  register(...cmds: Command[]): void {
    for (const c of cmds) this.static_.set(c.id, c);
  }

  addProvider(p: CommandProvider): void {
    this.providers.push(p);
  }

  /** Every currently available command; a later duplicate id is dropped. */
  all(): Command[] {
    const seen = new Map<string, Command>();
    for (const c of this.static_.values()) seen.set(c.id, c);
    for (const p of this.providers) {
      for (const c of p()) if (!seen.has(c.id)) seen.set(c.id, c);
    }
    return [...seen.values()].filter((c) => c.when?.() ?? true);
  }

  /** Commands whose title or keywords contain every word of `query`. */
  search(query: string, max = 40): Command[] {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const all = this.all();
    if (words.length === 0) return all.slice(0, max);
    const score = (c: Command): number => {
      const hay = `${c.title} ${c.keywords ?? ''} ${c.group}`.toLowerCase();
      if (!words.every((w) => hay.includes(w))) return -1;
      return c.title.toLowerCase().startsWith(words[0]) ? 0 : 1;
    };
    return all
      .map((c) => ({ c, s: score(c) }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => a.s - b.s || a.c.title.localeCompare(b.c.title))
      .slice(0, max)
      .map((x) => x.c);
  }
}

export const commands = new CommandRegistry();
