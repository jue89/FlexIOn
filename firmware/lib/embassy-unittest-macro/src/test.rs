use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, ItemFn, Result, parse2, spanned::Spanned as _};

pub fn test_impl(input: TokenStream) -> Result<TokenStream> {
    let span = input.span();

    // Ensure we got a async function
    let mut test_fn = parse2::<ItemFn>(input)?;
    if test_fn.sig.asyncness.is_none() {
        return Err(Error::new(span, "Test function must be async"));
    }

    let name = test_fn.sig.ident.clone();

    // Split-off fn attributes to move then to the created test fn
    let attr = test_fn.attrs.split_off(0);

    Ok(quote!(
        #[test]
        #(#attr)*
        fn #name() {
            use ::core::sync::atomic::{AtomicBool, Ordering};
            use ::embassy_unittest::static_cell::StaticCell;
            use ::embassy_unittest::embassy_executor::{task, Executor, Spawner};

            static DONE: AtomicBool = AtomicBool::new(false);
            static EXEC: StaticCell<Executor> = StaticCell::new();
            EXEC.init(Executor::new())
                .run_until(
                    |spawner| spawner.spawn(test().unwrap()),
                    || DONE.load(Ordering::Acquire),
                );

            #test_fn

            #[task(embassy_executor="::embassy_unittest::embassy_executor")]
            async fn test() {
                #name().await;
                DONE.store(true, Ordering::Release);
            }
        }
    ))
}
