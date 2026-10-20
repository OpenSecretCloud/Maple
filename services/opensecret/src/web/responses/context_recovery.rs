//! Request-local history reduction after a provider rejects an oversized prompt.
//!
//! Serialized size is only a reduction heuristic, not a token count or proof of
//! fit. Neither helper changes canonical conversation history or required input.

use serde_json::Value;
use std::collections::HashMap;

/// Remove roughly half of the removable history by serialized size. Prefer old
/// middle turns, retaining the first historical user turn when the other turns
/// are large enough to make the reduction. Return the adjusted required boundary.
pub(super) fn reduce_optional_history(
    messages: &[Value],
    required_from: usize,
) -> Option<(Vec<Value>, usize)> {
    reduce_history(messages, required_from, false)
}

/// Final bounded recovery attempt: drop all safely removable history, including
/// the first historical user turn. Instructions and the required suffix remain.
pub(super) fn drop_optional_history(
    messages: &[Value],
    required_from: usize,
) -> Option<(Vec<Value>, usize)> {
    reduce_history(messages, required_from, true)
}

#[derive(Debug)]
struct HistoryGroup {
    start: usize,
    end: usize,
    weight: usize,
    protected: bool,
}

#[derive(Debug)]
struct ToolRelation {
    first: usize,
    last: usize,
    pending_calls: usize,
}

fn is_instruction(message: &Value) -> bool {
    matches!(
        message.get("role").and_then(Value::as_str),
        Some("system" | "developer")
    )
}

/// Identify complete tool relations without interpreting or rewriting content.
/// Unrecognized/malformed relations are left to the existing provider path;
/// declining recovery is safer than making a new orphan while trimming them.
fn tool_spans(messages: &[Value]) -> Option<Vec<(usize, usize)>> {
    let mut relations: HashMap<&str, ToolRelation> = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        let object = message.as_object()?;
        let role = object.get("role")?.as_str()?;
        if !matches!(role, "system" | "developer" | "user" | "assistant" | "tool")
            || object.get("function_call").is_some_and(|v| !v.is_null())
        {
            return None;
        }
        if role != "assistant" && object.get("tool_calls").is_some_and(|v| !v.is_null()) {
            return None;
        }
        if role != "tool" && object.get("tool_call_id").is_some_and(|v| !v.is_null()) {
            return None;
        }

        if role == "assistant" {
            if let Some(calls) = object.get("tool_calls").filter(|value| !value.is_null()) {
                for call in calls.as_array()? {
                    let id = call.get("id")?.as_str().filter(|id| !id.is_empty())?;
                    let relation = relations.entry(id).or_insert(ToolRelation {
                        first: index,
                        last: index,
                        pending_calls: 0,
                    });
                    relation.last = index;
                    relation.pending_calls = relation.pending_calls.checked_add(1)?;
                }
            }
        } else if role == "tool" {
            let id = object
                .get("tool_call_id")?
                .as_str()
                .filter(|id| !id.is_empty())?;
            let relation = relations.get_mut(id)?;
            relation.pending_calls = relation.pending_calls.checked_sub(1)?;
            relation.last = index;
        }
    }
    relations
        .into_values()
        .map(|relation| (relation.pending_calls == 0).then_some((relation.first, relation.last)))
        .collect()
}

fn history_groups(messages: &[Value], required_from: usize) -> Option<Vec<HistoryGroup>> {
    if messages.get(required_from)?.get("role")?.as_str()? != "user" {
        return None;
    }
    let spans = tool_spans(messages)?;
    let starts: Vec<usize> = messages[..required_from]
        .iter()
        .enumerate()
        .filter_map(|(index, message)| (message["role"] == "user").then_some(index))
        .collect();
    let first_start = *starts.first()?;
    let mut groups: Vec<HistoryGroup> = starts
        .iter()
        .enumerate()
        .map(|(index, &start)| HistoryGroup {
            start,
            end: starts.get(index + 1).copied().unwrap_or(required_from),
            weight: 0,
            protected: false,
        })
        .collect();
    let mut joined_boundaries = vec![false; groups.len().saturating_sub(1)];
    let mut group_for_message = vec![None; required_from];
    for (group_index, group) in groups.iter().enumerate() {
        group_for_message[group.start..group.end].fill(Some(group_index));
    }

    // A repeated ID spans every occurrence, not only its latest call/result
    // pair. Joining all intervening user turns also handles overlapping spans.
    for (first, last) in spans {
        let first_group = (first < required_from)
            .then(|| group_for_message[first])
            .flatten();
        let last_group = (last < required_from)
            .then(|| group_for_message[last])
            .flatten();
        match (first_group, last_group) {
            (Some(first), Some(last)) => joined_boundaries[first..last].fill(true),
            (None, Some(last)) => {
                for group in &mut groups[..=last] {
                    group.protected = true;
                }
            }
            (Some(first), None) => {
                for group in &mut groups[first..] {
                    group.protected = true;
                }
            }
            (None, None) if first < first_start && last >= required_from => {
                for group in &mut groups {
                    group.protected = true;
                }
            }
            (None, None) => {}
        }
    }

    let mut atomic_groups: Vec<HistoryGroup> = Vec::new();
    for (index, group) in groups.into_iter().enumerate() {
        if index > 0 && joined_boundaries[index - 1] {
            let previous = atomic_groups.last_mut()?;
            previous.end = group.end;
            previous.protected |= group.protected;
        } else {
            atomic_groups.push(group);
        }
    }
    for group in &mut atomic_groups {
        for message in &messages[group.start..group.end] {
            if !is_instruction(message) {
                group.weight = group
                    .weight
                    .saturating_add(serde_json::to_vec(message).ok()?.len());
            }
        }
    }
    Some(atomic_groups)
}

fn reduce_history(
    messages: &[Value],
    required_from: usize,
    drop_all: bool,
) -> Option<(Vec<Value>, usize)> {
    let groups = history_groups(messages, required_from)?;
    let removable_weight = groups
        .iter()
        .filter(|group| !group.protected)
        .fold(0usize, |total, group| total.saturating_add(group.weight));
    if removable_weight == 0 {
        return None;
    }
    let target = removable_weight / 2 + removable_weight % 2;
    let mut removed_weight = 0usize;
    let mut remove = vec![false; required_from];
    // The first group contains the first historical user. Defer it unless the
    // other optional groups cannot meet the reduction target, or this is the
    // final attempt. Keep the most recent optional turns when possible.
    for group in groups.iter().skip(1).chain(groups.first()) {
        if group.protected || (!drop_all && removed_weight >= target) {
            continue;
        }
        for (index, message) in messages[group.start..group.end].iter().enumerate() {
            if !is_instruction(message) {
                remove[group.start + index] = true;
            }
        }
        removed_weight = removed_weight.saturating_add(group.weight);
    }
    if !remove.iter().any(|&removed| removed) {
        return None;
    }
    let adjusted_required_from = remove.iter().filter(|&&removed| !removed).count();
    let reduced = messages
        .iter()
        .enumerate()
        .filter(|(index, _)| *index >= required_from || !remove[*index])
        .map(|(_, message)| message.clone())
        .collect();
    Some((reduced, adjusted_required_from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn turn(label: &str) -> Vec<Value> {
        vec![
            json!({"role": "user", "content": format!("{label}: {}", "u".repeat(80))}),
            json!({"role": "assistant", "content": format!("{label}: {}", "a".repeat(80))}),
        ]
    }

    fn tool_turn(label: &str, id: &str) -> Vec<Value> {
        vec![
            json!({"role": "user", "content": label}),
            json!({"role": "assistant", "tool_calls": [{"id": id, "type": "function", "function": {"name": "lookup", "arguments": "{}"}}]}),
            json!({"role": "tool", "tool_call_id": id, "content": "result"}),
        ]
    }

    fn serialized_weight(messages: &[Value]) -> usize {
        messages
            .iter()
            .filter(|message| !is_instruction(message))
            .map(|message| serde_json::to_vec(message).unwrap().len())
            .sum()
    }

    #[test]
    fn first_recovery_halves_optional_weight_and_prefers_old_middle_turns() {
        let mut messages = vec![json!({"role": "system", "content": "instructions"})];
        for label in ["zero", "one!", "two!", "three", "four"] {
            messages.extend(turn(label));
        }
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));
        let original = messages.clone();
        let (reduced, boundary) = reduce_optional_history(&messages, required_from).unwrap();

        assert_eq!(messages, original);
        assert_eq!(&reduced[boundary..], &original[required_from..]);
        assert_eq!(reduced[0], original[0]);
        assert_eq!(&reduced[1..3], &original[1..3]);
        assert_eq!(
            &reduced[boundary - 2..boundary],
            &original[required_from - 2..required_from]
        );
        let before = serialized_weight(&messages[..required_from]);
        let after = serialized_weight(&reduced[..boundary]);
        assert!(before - after >= before / 2 + before % 2);
        assert!(boundary < required_from);
    }

    #[test]
    fn later_tool_turn_keeps_the_complete_required_suffix_byte_for_byte() {
        let mut messages = turn("old");
        messages.extend(turn("other"));
        let required_from = messages.len();
        messages.extend(tool_turn("current", "current-call"));
        messages.push(
            json!({"role": "assistant", "content": "after tool", "reasoning_content": "reasoning"}),
        );
        let suffix = messages[required_from..].to_vec();

        let (reduced, boundary) = reduce_optional_history(&messages, required_from).unwrap();
        assert_eq!(reduced[boundary..], suffix);
        let (final_reduced, final_boundary) = drop_optional_history(&reduced, boundary).unwrap();
        assert_eq!(final_boundary, 0);
        assert_eq!(final_reduced, suffix);
    }

    #[test]
    fn every_instruction_survives_even_inside_a_removed_user_turn() {
        let system = json!({"role": "system", "content": "first instruction"});
        let developer = json!({"role": "developer", "content": "middle instruction"});
        let later_system = json!({"role": "system", "content": "later instruction"});
        let mut messages = vec![system.clone()];
        messages.extend(turn("old"));
        messages.push(developer.clone());
        messages.extend(turn("other"));
        messages.push(later_system.clone());
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));
        let original = messages.clone();

        let (reduced, boundary) = drop_optional_history(&messages, required_from).unwrap();
        assert_eq!(&reduced[..boundary], &[system, developer, later_system]);
        assert_eq!(&reduced[boundary..], &messages[required_from..]);
        assert_eq!(messages, original);
    }

    #[test]
    fn a_huge_optional_turn_is_removed_atomically_even_when_it_is_first() {
        let mut messages = vec![json!({"role": "user", "content": "huge".repeat(20_000)})];
        messages.push(json!({"role": "assistant", "content": "answer"}));
        messages.extend(turn("tiny"));
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));

        let (reduced, boundary) = reduce_optional_history(&messages, required_from).unwrap();
        assert_eq!(boundary, 0);
        assert_eq!(reduced, messages[required_from..]);
    }

    #[test]
    fn repeated_interleaved_tool_ids_join_every_intervening_turn() {
        let mut messages = tool_turn("old zero", "a");
        messages.extend(tool_turn("old one", "b"));
        messages.extend(tool_turn("old two", "a"));
        messages.extend(tool_turn("old three", "b"));
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));

        let (reduced, boundary) = reduce_optional_history(&messages, required_from).unwrap();
        assert_eq!(boundary, 0);
        assert_eq!(reduced, messages[required_from..]);
        assert!(tool_spans(&reduced).is_some());
    }

    #[test]
    fn a_tool_relation_into_the_required_suffix_protects_its_optional_ancestors() {
        let mut messages = turn("removable");
        let protected_start = messages.len();
        messages.push(json!({"role": "user", "content": "old lookup"}));
        messages.push(json!({"role": "assistant", "tool_calls": [{"id": "cross", "function": {"name": "lookup", "arguments": "{}"}}]}));
        messages.extend(turn("intervening"));
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));
        messages.push(json!({"role": "tool", "tool_call_id": "cross", "content": "result"}));

        let (reduced, boundary) = drop_optional_history(&messages, required_from).unwrap();
        assert_eq!(reduced, messages[protected_start..]);
        assert_eq!(boundary, required_from - protected_start);
        assert!(tool_spans(&reduced).is_some());
        assert!(drop_optional_history(&reduced, boundary).is_none());
    }

    #[test]
    fn a_tool_relation_from_a_non_user_prelude_protects_the_linked_turn() {
        let mut messages = vec![json!({"role": "assistant", "tool_calls": [{"id": "prelude"}]})];
        messages.push(json!({"role": "user", "content": "first historical user"}));
        messages.push(json!({"role": "tool", "tool_call_id": "prelude", "content": "result"}));
        let protected_end = messages.len();
        messages.extend(turn("removable"));
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));

        let (reduced, boundary) = drop_optional_history(&messages, required_from).unwrap();
        assert_eq!(&reduced[..boundary], &messages[..protected_end]);
        assert_eq!(&reduced[boundary..], &messages[required_from..]);
        assert!(tool_spans(&reduced).is_some());
    }

    #[test]
    fn a_prelude_to_required_suffix_relation_protects_every_optional_turn() {
        let mut messages = vec![json!({"role": "assistant", "tool_calls": [{"id": "cross"}]})];
        messages.extend(turn("historical zero"));
        messages.extend(turn("historical one"));
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));
        messages.push(json!({"role": "tool", "tool_call_id": "cross", "content": "result"}));
        let original = messages.clone();

        assert!(reduce_optional_history(&messages, required_from).is_none());
        assert!(drop_optional_history(&messages, required_from).is_none());
        assert_eq!(messages, original);
    }

    #[test]
    fn overlapping_spans_propagate_suffix_protection_across_merged_groups() {
        let messages = vec![
            json!({"role": "user", "content": "first historical user"}),
            json!({"role": "assistant", "tool_calls": [{"id": "history"}]}),
            json!({"role": "user", "content": "second historical user"}),
            json!({"role": "assistant", "tool_calls": [{"id": "suffix"}]}),
            json!({"role": "user", "content": "third historical user"}),
            json!({"role": "tool", "tool_call_id": "history", "content": "result"}),
            json!({"role": "user", "content": "current"}),
            json!({"role": "tool", "tool_call_id": "suffix", "content": "result"}),
        ];
        let required_from = 6;
        let original = messages.clone();

        assert!(reduce_optional_history(&messages, required_from).is_none());
        assert!(drop_optional_history(&messages, required_from).is_none());
        assert_eq!(messages, original);
    }

    #[test]
    fn non_user_prelude_is_retained_without_turn_inference() {
        let prelude =
            json!({"role": "assistant", "content": "prior context with no user boundary"});
        let mut messages = vec![prelude.clone()];
        messages.extend(turn("removable"));
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));

        let (reduced, boundary) = drop_optional_history(&messages, required_from).unwrap();
        assert_eq!(boundary, 1);
        assert_eq!(reduced[0], prelude);
        assert_eq!(&reduced[boundary..], &messages[required_from..]);
    }

    #[test]
    fn invalid_boundaries_and_history_without_optional_user_turns_do_not_recover() {
        let messages = vec![
            json!({"role": "system", "content": "instructions"}),
            json!({"role": "user", "content": "current"}),
        ];
        for required_from in [0, 1, 2, usize::MAX] {
            assert!(reduce_optional_history(&messages, required_from).is_none());
            assert!(drop_optional_history(&messages, required_from).is_none());
        }
        assert!(reduce_optional_history(&[], 0).is_none());
    }

    #[test]
    fn malformed_or_incomplete_tool_relations_decline_recovery_without_mutation() {
        let malformed = [
            json!({"role": "tool", "tool_call_id": "orphan", "content": "result"}),
            json!({"role": "assistant", "tool_calls": [{"id": "missing-result"}]}),
            json!({"role": "assistant", "tool_calls": [{"id": ""}]}),
            json!({"role": "assistant", "tool_calls": {"id": "wrong-shape"}}),
            json!({"role": "assistant", "function_call": {"name": "legacy"}}),
            json!({"role": "user", "tool_call_id": "unexpected"}),
            json!({"role": "custom", "content": "unknown role"}),
            json!("not a message"),
        ];
        for malformed_message in malformed {
            let mut messages = turn("old");
            messages.push(malformed_message);
            let required_from = messages.len();
            messages.push(json!({"role": "user", "content": "current"}));
            let original = messages.clone();
            assert!(reduce_optional_history(&messages, required_from).is_none());
            assert!(drop_optional_history(&messages, required_from).is_none());
            assert_eq!(messages, original);
        }
    }

    #[test]
    fn an_incomplete_required_tool_suffix_is_conservatively_unchanged() {
        let mut messages = turn("old");
        let required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));
        messages.push(json!({"role": "assistant", "tool_calls": [{"id": "pending"}]}));
        let original = messages.clone();

        assert!(reduce_optional_history(&messages, required_from).is_none());
        assert!(drop_optional_history(&messages, required_from).is_none());
        assert_eq!(messages, original);
    }

    #[test]
    fn each_reduction_makes_finite_progress_and_the_final_attempt_drops_the_rest() {
        let instruction = json!({"role": "developer", "content": "instructions"});
        let mut messages = vec![instruction.clone()];
        for index in 0..17 {
            messages.extend(turn(&format!("turn {index}")));
        }
        let mut required_from = messages.len();
        messages.push(json!({"role": "user", "content": "current"}));
        let original = messages.clone();
        let suffix = messages[required_from..].to_vec();
        let (first, first_boundary) = reduce_optional_history(&messages, required_from).unwrap();
        let (final_reduced, final_boundary) =
            drop_optional_history(&first, first_boundary).unwrap();
        assert_eq!(final_boundary, 1);
        assert_eq!(final_reduced, [vec![instruction], suffix.clone()].concat());

        let mut attempts = 0;
        while let Some((reduced, boundary)) = reduce_optional_history(&messages, required_from) {
            assert!(reduced.len() < messages.len());
            assert!(serialized_weight(&reduced) < serialized_weight(&messages));
            assert_eq!(reduced[boundary..], suffix);
            messages = reduced;
            required_from = boundary;
            attempts += 1;
            assert!(attempts <= 17);
        }
        assert_eq!(required_from, 1);
        assert_eq!(original.last(), messages.last());
    }
}
