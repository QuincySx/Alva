const { chromium } = require('/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-hub-desktop/web/node_modules/playwright');
const path = require('path');
const fs = require('fs');

async function main() {
  const outDir = path.resolve(__dirname, 'screenshots_live');
  if (!fs.existsSync(outDir)) {
    fs.mkdirSync(outDir, { recursive: true });
  }

  const browser = await chromium.launch({
    executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    headless: true,
  });

  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
  });

  const page = await context.newPage();
  await page.goto('http://127.0.0.1:8790/?demo=1', { waitUntil: 'networkidle' });

  // 1. Home Conversation
  await page.screenshot({ path: path.join(outDir, '01-home.png') });

  // 2. Settings Page - General
  await page.click('text=设置');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '02-settings-general.png') });

  // 3. Settings Page - Models
  await page.click('text=模型供应商');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '03-settings-models.png') });

  // 4. Settings Page - Tools & MCP
  await page.click('text=内置工具与 MCP');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '04-settings-tools.png') });

  // 5. Settings Page - Remote SSH
  await page.click('text=远程 SSH 连接');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '05-settings-remote.png') });

  // 6. Settings Page - IM Gateways
  await page.click('text=IM 机器人');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '06-settings-im.png') });

  // 7. Settings Page - Keybindings
  await page.click('text=键盘快捷键');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '07-settings-keybindings.png') });

  // 8. Settings Page - Usage
  await page.click('text=用量与计费');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '08-settings-usage.png') });

  // 9. Settings Page - Permissions
  await page.click('text=权限与安全');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '09-settings-permissions.png') });

  // 10. Settings Page - Help & About
  await page.click('text=关于与更新');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '10-settings-help.png') });

  // 11. Automations Page
  await page.click('text=自动化');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '11-automations.png') });

  // 12. Plugins/Extensions Page
  await page.click('text=插件');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '12-extensions.png') });

  // 13. Search Page
  await page.click('text=搜索');
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(outDir, '13-search.png') });

  await browser.close();
  console.log('All screenshots captured successfully in', outDir);
}

main().catch(err => {
  console.error('Error capturing screenshots:', err);
  process.exit(1);
});
