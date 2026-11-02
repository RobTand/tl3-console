import { render, screen } from "@testing-library/react";
import { IssueLink, issueUrl } from "./IssueLink";

describe("issueUrl", () => {
  it("is the issue's GitHub page", () => {
    expect(issueUrl("RobTand/tl3-sandbox", 42)).toBe("https://github.com/RobTand/tl3-sandbox/issues/42");
    expect(issueUrl("RobTand/prisma.quant_2", 7)).toBe("https://github.com/RobTand/prisma.quant_2/issues/7");
  });

  it("refuses anything that isn't an owner and a repository name", () => {
    for (const repo of [
      "",
      "tl3-sandbox",
      "a/b/c",
      "evil.example/../x",
      "RobTand/..",
      "./tl3-sandbox",
      "RobTand/tl3 sandbox",
      "RobTand/tl3-sandbox?x=1",
      "javascript:alert(1)//x",
    ]) {
      expect(issueUrl(repo, 1), repo).toBeNull();
    }
  });
});

describe("IssueLink", () => {
  it("links the reference to the issue in a new tab", () => {
    render(<IssueLink repo="RobTand/tl3-sandbox" issue={42} short />);
    const link = screen.getByRole("link", { name: "tl3-sandbox#42" });
    expect(link).toHaveAttribute("href", "https://github.com/RobTand/tl3-sandbox/issues/42");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noreferrer");
    expect(link).toHaveAttribute("title", "Open RobTand/tl3-sandbox#42 on GitHub");
  });

  it("names the owner too unless told to be short", () => {
    render(<IssueLink repo="RobTand/tl3-sandbox" issue={42} />);
    expect(screen.getByRole("link", { name: "RobTand/tl3-sandbox#42" })).toBeVisible();
  });

  it("shows a reference it can't build a link for as plain text", () => {
    render(<IssueLink repo="javascript:alert(1)//x" issue={1} />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.getByText("javascript:alert(1)//x")).toBeVisible();
    expect(screen.getByText("#1")).toBeVisible();
  });
});
