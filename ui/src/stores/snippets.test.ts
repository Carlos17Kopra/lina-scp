import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { makeSnippet } from "../test/fixtures";
import { useSnippetsStore } from "./snippets";

vi.mock("../ipc/api");

beforeEach(() => {
  useSnippetsStore.setState(useSnippetsStore.getInitialState(), true);
});

describe("snippets store", () => {
  it("reloads the list after save and remove", async () => {
    const snippet = makeSnippet();
    vi.mocked(api.snippetSave).mockResolvedValue(snippet);
    vi.mocked(api.snippetsList).mockResolvedValueOnce([snippet]).mockResolvedValueOnce([]);

    await expect(useSnippetsStore.getState().save(snippet)).resolves.toEqual(snippet);
    expect(useSnippetsStore.getState()).toMatchObject({ snippets: [snippet], loaded: true });

    await useSnippetsStore.getState().remove(snippet.id);
    expect(api.snippetDelete).toHaveBeenCalledWith(snippet.id);
    expect(useSnippetsStore.getState().snippets).toEqual([]);
  });
});
