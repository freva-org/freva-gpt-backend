use itertools::Itertools;
use tracing::{debug, info, trace, warn};

use crate::chatbot::types::{variant_name, StreamVariant};

/// Takes in the past variants from the frontend as well as the past variants from the thread,
/// and returns those variants that the frontend hinted it wanted to continue from.
pub fn filter_variants(
    frontend_variants: &str,
    storage_variants: Vec<StreamVariant>,
) -> Result<Vec<StreamVariant>, String> {
    // First, we'll need to extracts the variants names from the frontend string.
    trace!("Recieved from frontend: {frontend_variants}");

    // We'll assume they are some kind of comma-separated list, so we'll remove all whitespace, quotes, brackets, and split by commas.
    let frontend_variants: Vec<String> = frontend_variants
        .replace(" ", "")
        .replace("\"", "")
        .replace("\'", "")
        .replace("[", "")
        .replace("]", "")
        .split(',')
        .map(|s| s.to_string())
        .collect();

    trace!("Converted to List of variants: {:?}", frontend_variants);

    // The frontend may not deduplicate consecutive variants, so we'll deduplicate them.
    let frontend_variants: Vec<String> = frontend_variants.into_iter().dedup().collect();

    // The backend variants may contain the prompt, so we'll filter it out.
    let storage_variants: Vec<StreamVariant> = storage_variants
        .into_iter()
        .filter(|v| !matches!(v, StreamVariant::Prompt(_)))
        .collect();

    // Start by trying to match, starting at the beginning.
    match matches_variants(&frontend_variants, &storage_variants, true, &[]) {
        Some(matched_variants) => {
            trace!("Matched variants: {:?}", matched_variants);
            return Ok(matched_variants.to_vec());
        }
        None => {
            trace!("No matching variants found.");
        }
    }
    info!("No matching variants found in first iteration, trying to ignore some variants.");

    match matches_variants(
        &frontend_variants,
        &storage_variants,
        true,                      // Start at the beginning
        &["Prompt", "ServerHint"], // Ignore the "Prompt" variant, something might have gone wrong. The ServerHint might get ignored by the frontend.
    ) {
        Some(matched_variants) => {
            trace!(
                "Matched variants while ignoring Prompt and ServerHint: {:?}",
                matched_variants
            );
            return Ok(matched_variants.to_vec());
        }
        None => {
            trace!("No matching variants found while ignoring Prompt and Serverhint.");
        }
    }
    info!("No matching variants found in second iteration, trying to ignore even more variants.");
    // Next, we'll ignore all Error and Ending variants, as the frontend might ignore them.
    match matches_variants(
        &frontend_variants,
        &storage_variants,
        true, // Start at the beginning
        &[
            "Prompt",
            "ServerHint",
            "ServerError",
            "OpenAIError",
            "CodeError",
            "StreamEnd",
        ],
    ) {
        Some(matched_variants) => {
            trace!("Matched variants while ignoring Prompt, ServerHint, ServerError, OpenAIError, CodeError, and StreamEnd: {:?}", matched_variants);
            return Ok(matched_variants.to_vec());
        }
        None => {
            trace!("No matching variants found while ignoring Prompt, ServerHint, ServerError, OpenAIError, CodeError, and StreamEnd.");
        }
    }
    warn!("No matching variants found starting at the beginning, trying to match from anywhere.");
    // Finally, we'll try to match from anywhere, first without ignoring any variants.
    match matches_variants(&frontend_variants, &storage_variants, false, &[]) {
        Some(matched_variants) => {
            trace!("Matched variants from anywhere: {:?}", matched_variants);
            return Ok(matched_variants.to_vec());
        }
        None => {
            trace!("No matching variants found from anywhere.");
        }
    }
    info!("No matching variants found in fourth iteration, trying to ignore some variants.");
    // Next, we'll try to match from anywhere, ignoring the "Prompt" variant.
    match matches_variants(
        &frontend_variants,
        &storage_variants,
        false, // Start at anywhere
        &["Prompt", "ServerHint"],
    ) {
        Some(matched_variants) => {
            trace!(
                "Matched variants from anywhere while ignoring Prompt and ServerHint: {:?}",
                matched_variants
            );
            return Ok(matched_variants.to_vec());
        }
        None => {
            trace!(
                "No matching variants found from anywhere while ignoring Prompt and ServerHint."
            );
        }
    }
    info!("No matching variants found in fifth iteration, trying to ignore even more variants.");
    // Finally, we'll try to match from anywhere, ignoring all Error and Ending variants.
    match matches_variants(
        &frontend_variants,
        &storage_variants,
        false, // Start at anywhere
        &[
            "Prompt",
            "ServerHint",
            "ServerError",
            "OpenAIError",
            "CodeError",
            "StreamEnd",
        ],
    ) {
        Some(matched_variants) => {
            trace!("Matched variants from anywhere while ignoring Prompt, ServerHint, ServerError, OpenAIError, CodeError, and StreamEnd: {:?}", matched_variants);
            return Ok(matched_variants.to_vec());
        }
        None => {
            trace!("No matching variants found from anywhere while ignoring Prompt, ServerHint, ServerError, OpenAIError, CodeError, and StreamEnd.");
        }
    }
    // If we reach here, we couldn't match any variants.
    // Ignoring any more variants is likely not going to help, so we'll just return an error.
    warn!("No matching variants found after all iterations. This likely means the frontend sent an edit-input that doesn't match the current conversation.");
    Err("No matching variants found".to_string())
}

/// Tries to match the given variant names, with settings of having to either start at the start or not,
/// and also whether or not to maybe ignore specific variants.
fn matches_variants(
    frontend_variants: &[String],
    storage_variants: &[StreamVariant],
    start_at_beginning: bool,
    ignore_variants: &[&str],
) -> Option<Vec<StreamVariant>> {
    // This function implements four (or five, depending on how you count) different matching strategies:
    // 1. Match from the start, matching all variants in order.
    // 2. Match from the start, ignoring specific variants.
    // 3. Match from anywhere, matching all variants in order.
    // 4. Match from anywhere, ignoring specific variants.
    // (5. Match from the start, but allow for all variants to be ignored. This shouldn't happen, but is a fallback.)

    // To facilitate this, we'll use iterators.

    // The potential starting points
    let starting_points = if start_at_beginning {
        // If we are starting at the beginning, we only have one starting point.
        vec![0]
    } else {
        // If we are not starting at the beginning, we can start at any point in the storage variants.
        (0..storage_variants.len()).collect()
    };

    // Debug: better debugging output.
    let debug_variants = storage_variants
        .iter()
        .map(variant_name)
        .collect::<Vec<_>>();
    debug!("Storage variants: {:?}", debug_variants);
    debug!("Frontend variants: {:?}", frontend_variants);

    for start in starting_points {
        let mut s_iter = storage_variants
            .iter()
            .skip(start) // Skip to the starting point
            .filter(|v| !ignore_variants.contains(&variant_name(v).as_str()));
        let f_iter = frontend_variants
            .iter()
            .filter(|v| !ignore_variants.contains(&v.as_str()));

        let mut return_variants = Vec::new();
        let mut succeeded = true;
        // Instead of zipping, we'll iterate over the f iter, because that should be the shorter one.
        for f_variant in f_iter {
            // If we run out of storage variants, we can't match anymore.
            let Some(s_variant) = s_iter.next() else {
                info!("Ran out of storage variants while matching. This means the frontend likely sent an edit before the streaming was done; this isn't planned currently.");
                succeeded = false;
                break;
            };
            // Check whether the variant matches.
            let variant_name = variant_name(s_variant);
            if variant_name != *f_variant {
                debug!(
                    "Variant mismatch: expected {}, got {}",
                    f_variant, variant_name
                );
                // They definitely don't match, so we can stop this iteration.
                succeeded = false;
                break;
            }
            // They match, we can continue. But we need to store the variant.
            return_variants.push(s_variant.clone());
        }
        // If we succeeded, we can return the variants.
        if succeeded {
            debug!("Matched variants: {:?}", return_variants);
            return Some(return_variants);
        }
        // Else, we continue to the next starting point.
    }
    // If we reach here, we tried all starting points and none matched.
    info!("No matching variants found. The edit-input from the frontend likely doesn't match the current conversation.");
    None
}
