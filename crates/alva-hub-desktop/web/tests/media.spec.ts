// INPUT: Real Chrome/Vite UI Lab and intercepted read-only HTTP media fixtures
// OUTPUT: Keyboard/Blob decode/focus/locale/viewport/no-mutation browser regressions
// POS: Deterministic browser evidence for media inspection, not native WebView or provider acceptance.
import { expect, test } from "@playwright/test";

for (const language of ["en-US", "zh-CN"]) for (const width of [1440, 720]) {
  test(`media inspection: ${language} ${width}px, real local image and keyboard lifecycle`, async ({page}) => {
    await page.setViewportSize({width,height:900});
    await page.addInitScript((locale) => {
      localStorage.setItem("alva-hub.language",locale);
      const originalCreate=URL.createObjectURL.bind(URL), originalRevoke=URL.revokeObjectURL.bind(URL);
      const counts={created:0,revoked:0};
      Object.assign(window,{mediaPreviewCounts:counts});
      URL.createObjectURL=(blob)=>{counts.created++;return originalCreate(blob);};
      URL.revokeObjectURL=(url)=>{counts.revoked++;originalRevoke(url);};
    },language);
    // A synthetic JPEG from Chrome itself: no external asset or native decoder.
    const encoded=await page.evaluate(()=>{const canvas=document.createElement("canvas");canvas.width=2;canvas.height=2;canvas.getContext("2d")!.fillRect(0,0,2,2);return canvas.toDataURL("image/jpeg").split(",")[1];});
    const artifact={artifact_id:"fixture-jpeg",source_attachment_id:"fixture-source",name:"derived.jpg",total_bytes:Buffer.from(encoded,"base64").length,path:"media/derived.jpg",content_type:"image/jpeg",processing:{output_sha256:"fixture-sha",source_sha256:"source-sha",method:"browser_fixture_only",actual_timestamp_seconds:0.6,selection_reason:"<script>not executable</script>",original_preserved:true}};
    const requests:string[]=[];
    await page.route("**/api/**",async (route)=>{
      requests.push(`${route.request().method()} ${new URL(route.request().url()).pathname}`);
      const url=new URL(route.request().url());
      let value:unknown;
      if (url.pathname.endsWith("/media")) value={artifacts:[artifact],next_cursor:1,has_more:false};
      else if (url.pathname.endsWith("/receipts")) value={receipts:[{delivery_id:"fixture-receipt",status:"interrupted_unknown",created_at:1,updated_at:2,selection:{actual_model_id:"fixture-model",originals_sent:false}}],next_cursor:1,through:1,has_more:false};
      else if (url.pathname.endsWith("/fixture-jpeg")) value={...artifact,data_base64:encoded};
      else throw new Error(`Unexpected inspection API: ${url.pathname}`);
      await route.fulfill({contentType:"application/json",body:JSON.stringify(value)});
    });
    await page.goto("/?ui-lab=1");
    await page.getByRole("tab",{name:"Media evidence"}).click();
    const trigger=page.getByRole("button",{name:language==="zh-CN"?"查看 fixture-video.mp4 的处理与投递":"Inspect processing and delivery for fixture-video.mp4"});
    await trigger.focus();await page.keyboard.press("Enter");
    const dialog=page.getByRole("dialog");
    await expect(dialog).toContainText("actual_timestamp_seconds");
    expect(requests.every((r)=>r.startsWith("GET "))).toBe(true);
    expect(requests.some((r)=>r.endsWith("/fixture-jpeg"))).toBe(false);
    await dialog.getByRole("button",{name:/derived.jpg.*fixture-jpeg/}).click();
    const image=dialog.getByRole("img",{name:"derived.jpg"});
    await expect(image).toBeVisible();
    await expect.poll(()=>image.evaluate((img)=>(img as HTMLImageElement).naturalWidth)).toBe(2);
    await expect(dialog).toContainText(language==="zh-CN"?"投递是否被接受未知":"Delivery acceptance unknown");
    await expect(dialog.locator("script")).toHaveCount(0);
    expect(await dialog.evaluate((node)=>node.scrollWidth-node.clientWidth)).toBeLessThanOrEqual(0);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();await expect(trigger).toBeFocused();
    await expect.poll(()=>page.evaluate(()=>{const counts=(window as unknown as {mediaPreviewCounts:{created:number;revoked:number}}).mediaPreviewCounts;return counts.created-counts.revoked;})).toBe(0);
    expect(requests.length).toBeGreaterThanOrEqual(3);expect(requests.every((r)=>r.startsWith("GET "))).toBe(true);
  });
}
