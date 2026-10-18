// INPUT: Pinned QuickJS core, bounded strict source and synchronous host capability imports
// OUTPUT: Fresh per-instance evaluator, bounded UTF-8 result/error and accurate heap accounting
// POS: [platform / WASM host] No native FFI/libc modules, loader, ambient filesystem or detached work
#include "quickjs.h"
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <stdalign.h>
#include "bootstrap.h"

__attribute__((import_module("alva_script"), import_name("call")))
extern uint32_t host_call(uint32_t operation, const char *data, uint32_t length);
__attribute__((import_module("alva_script"), import_name("read")))
extern uint32_t host_read(char *data, uint32_t length);
__attribute__((import_module("alva_script"), import_name("interrupted")))
extern uint32_t host_interrupted(void);

enum { HEAP_LIMIT = 32*1024*1024, MAX_RESULT = 64*1024, MAX_BRIDGE = 8*1024*1024 };
typedef union { struct { size_t size; } info; max_align_t alignment; } Header;
static size_t allocated;
static void *tracked_malloc(void *opaque, size_t size) {
    (void)opaque;
    if (size > HEAP_LIMIT-sizeof(Header) || allocated > HEAP_LIMIT-size-sizeof(Header)) return NULL;
    Header *h = malloc(sizeof(Header)+size);
    if (!h) return NULL;
    h->info.size=size; allocated+=size+sizeof(Header); return h+1;
}
static void tracked_free(void *opaque, void *ptr) {
    (void)opaque; if (!ptr) return;
    Header *h=(Header*)ptr-1; allocated-=h->info.size+sizeof(Header); free(h);
}
static void *tracked_calloc(void *opaque, size_t count, size_t size) {
    if (count && size > SIZE_MAX/count) return NULL;
    size*=count; void *ptr=tracked_malloc(opaque,size); if (ptr) memset(ptr,0,size); return ptr;
}
static void *tracked_realloc(void *opaque, void *ptr, size_t size) {
    if (!ptr) return tracked_malloc(opaque,size);
    if (!size) { tracked_free(opaque,ptr); return NULL; }
    Header *h=(Header*)ptr-1; size_t old=h->info.size;
    if (size > HEAP_LIMIT-sizeof(Header) || allocated-old > HEAP_LIMIT-size) return NULL;
    h=realloc(h,sizeof(Header)+size); if (!h) return NULL;
    h->info.size=size; allocated=allocated-old+size; return h+1;
}
static size_t tracked_size(const void *ptr) { return ptr ? ((const Header*)ptr-1)->info.size : 0; }
static int interrupt(JSRuntime *rt, void *opaque) { (void)rt; (void)opaque; return host_interrupted()!=0; }

static JSValue capability(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv, int op) {
    (void)self;
    JSValue array=JS_NewArray(ctx);
    if (JS_IsException(array)) return array;
    for (int i=0;i<argc;i++) if (JS_SetPropertyUint32(ctx,array,(uint32_t)i,JS_DupValue(ctx,argv[i]))<0) { JS_FreeValue(ctx,array);return JS_EXCEPTION; }
    JSValue json=JS_JSONStringify(ctx,array,JS_UNDEFINED,JS_UNDEFINED);JS_FreeValue(ctx,array);
    if (JS_IsException(json)) return json;
    size_t length=0;const char *text=JS_ToCStringLen(ctx,&length,json);JS_FreeValue(ctx,json);
    if (!text) return JS_EXCEPTION;
    if (length > MAX_BRIDGE) { JS_FreeCString(ctx,text);return JS_ThrowRangeError(ctx,"binding input exceeds 8MiB"); }
    uint32_t n=host_call((uint32_t)op,text,(uint32_t)length);JS_FreeCString(ctx,text);
    if (!n || n>MAX_BRIDGE) return JS_ThrowInternalError(ctx,"invalid host binding response");
    char *body=js_malloc(ctx,(size_t)n+1);if (!body) return JS_EXCEPTION;
    if (host_read(body,n)!=n) { js_free(ctx,body);return JS_ThrowInternalError(ctx,"incomplete host binding response"); }
    body[n]=0;JSValue response=JS_ParseJSON(ctx,body,n,"binding-result");js_free(ctx,body);
    if (JS_IsException(response)) return response;
    JSValue error=JS_GetPropertyStr(ctx,response,"error");
    if (!JS_IsUndefined(error)) {
        const char *message=JS_ToCString(ctx,error);JSValue result=JS_ThrowInternalError(ctx,"%s",message?message:"host binding rejected");
        if (message) JS_FreeCString(ctx,message);JS_FreeValue(ctx,error);JS_FreeValue(ctx,response);return result;
    }
    JS_FreeValue(ctx,error);JSValue result=JS_GetPropertyStr(ctx,response,"value");JS_FreeValue(ctx,response);return result;
}

static const char fallback[]="{\"error\":\"QuickJS allocation/evaluation failed\"}";
static char *result_data;
static uint32_t result_length;

__attribute__((export_name("alva_alloc")))
uint32_t alva_alloc(uint32_t length) { if (length>65536) return 0;return (uint32_t)(uintptr_t)malloc((size_t)length+1); }
__attribute__((export_name("alva_result_length")))
uint32_t alva_result_length(void) { return result_length; }
__attribute__((export_name("alva_eval")))
uint32_t alva_eval(uint32_t source, uint32_t length) {
    result_data=(char*)fallback;result_length=sizeof(fallback)-1;
    if (length>65536) return (uint32_t)(uintptr_t)result_data;
    ((char*)(uintptr_t)source)[length]=0;
    const JSMallocFunctions allocator={tracked_calloc,tracked_malloc,tracked_free,tracked_realloc,tracked_size};
    JSRuntime *rt=JS_NewRuntime2(&allocator,NULL);if (!rt) return (uint32_t)(uintptr_t)result_data;
    JS_SetMemoryLimit(rt,HEAP_LIMIT);JS_SetMaxStackSize(rt,512*1024);JS_SetCanBlock(rt,0);JS_SetInterruptHandler(rt,interrupt,NULL);
    JSContext *ctx=JS_NewContext(rt);if (!ctx) {JS_FreeRuntime(rt);return (uint32_t)(uintptr_t)result_data;}
    JSValue global=JS_GetGlobalObject(ctx),functions=JS_NewArray(ctx);
    for (int i=0;i<13;i++) JS_SetPropertyUint32(ctx,functions,(uint32_t)i,JS_NewCFunctionMagic(ctx,capability,"capability",0,JS_CFUNC_generic_magic,i));
    JS_SetPropertyStr(ctx,global,"__alva",functions);JS_FreeValue(ctx,global);
    JSValue value=JS_Eval(ctx,bootstrap,sizeof(bootstrap)-1,"alva-bindings",JS_EVAL_TYPE_GLOBAL|JS_EVAL_FLAG_STRICT);
    if (!JS_IsException(value)) {JS_FreeValue(ctx,value);JS_UpdateStackTop(rt);value=JS_Eval(ctx,(const char*)(uintptr_t)source,length,"run_script",JS_EVAL_TYPE_GLOBAL|JS_EVAL_FLAG_STRICT);}
    int failed=JS_IsException(value);
    if (failed) value=JS_GetException(ctx);
    size_t text_length=0;const char *text=JS_ToCStringLen(ctx,&text_length,value);JS_FreeValue(ctx,value);
    if (text && text_length<=MAX_RESULT/2) {
        JSValue object=JS_NewObject(ctx);
        JS_SetPropertyStr(ctx,object,failed?"error":"result",JS_NewStringLen(ctx,text,text_length));
        JSValue encoded=JS_JSONStringify(ctx,object,JS_UNDEFINED,JS_UNDEFINED);JS_FreeValue(ctx,object);
        size_t n=0;const char *json=JS_ToCStringLen(ctx,&n,encoded);JS_FreeValue(ctx,encoded);
        if (json && n<=MAX_RESULT) {char *copy=malloc(n+1);if (copy) {memcpy(copy,json,n+1);result_data=copy;result_length=(uint32_t)n;}}
        if (json) JS_FreeCString(ctx,json);
    }
    if (text) JS_FreeCString(ctx,text);JS_FreeContext(ctx);JS_FreeRuntime(rt);free((void*)(uintptr_t)source);
    return (uint32_t)(uintptr_t)result_data;
}
