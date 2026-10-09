// `pnpm debug web --script scripts/debug/web-scripts/pre-stream-failure.mjs`
// Fail the completion request (terminal 400) before any token streams and
// report what the UI shows. openhuman#5729.
export default async function preStreamFailure({ page, mock, screenshot, log }) {
  mock.set("httpFaultRules", [{ contains: "/chat/completions", mode: "status", status: 400 }]);
  const composer = page.getByRole("textbox", { name: "Message input" });
  await composer.click();
  await composer.pressSequentially("this turn dies before it streams");
  await page.getByRole("button", { name: "Send message" }).click();
  for (const s of [5, 15, 30]) {
    await page.waitForTimeout(s === 5 ? 5000 : 10000 + (s === 30 ? 5000 : 0));
    const banner = await page.locator("[data-chat-send-error-code]").count();
    const bubbles = await page.locator('[data-testid="agent-message"]').allInnerTexts();
    log(`t~${s}s banner=${banner} agentBubbles=${JSON.stringify(bubbles)}`);
    await screenshot(`t${s}`);
  }
}
