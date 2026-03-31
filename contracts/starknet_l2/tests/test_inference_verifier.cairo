// tests/test_inference_verifier.cairo

use snforge_std::{
    ContractClassTrait, DeclareResultTrait, EventSpyAssertionsTrait, L1Handler, L1HandlerTrait,
    declare, spy_events, start_cheat_caller_address, stop_cheat_caller_address,
};
use starknet::{ContractAddress, EthAddress, contract_address_const};
use starknet_l2::inference_verifier::{
    IInferenceVerifierDispatcher, IInferenceVerifierDispatcherTrait, InferenceRecord,
    InferenceVerifier,
};

// ── Helpers
// ───────────────────────────────────────────────────────────────────

fn OWNER() -> ContractAddress {
    contract_address_const::<0x1234>()
}

fn L1_BRIDGE() -> EthAddress {
    0xDeadBeefDeadBeefDeadBeefDeadBeefDeadBeef_u256.try_into().unwrap()
}

fn SECOND_BRIDGE() -> EthAddress {
    0xCafeC0ffeeCafeC0ffeeCafeC0ffeeCafeC0ffee_u256.try_into().unwrap()
}

fn deploy() -> IInferenceVerifierDispatcher {
    let contract = declare("InferenceVerifier").unwrap().contract_class();
    let mut calldata = array![];
    OWNER().serialize(ref calldata);
    L1_BRIDGE().serialize(ref calldata);

    let (addr, _) = contract.deploy(@calldata).unwrap();
    IInferenceVerifierDispatcher { contract_address: addr }
}

// ── Tests
// ─────────────────────────────────────────────────────────────────────

#[test]
fn test_deploy_sets_owner() {
    let d = deploy();
    assert_eq!(d.get_owner(), OWNER());
}

#[test]
fn test_initial_l1_verifier_whitelisted() {
    let d = deploy();
    assert!(d.is_l1_verifier(L1_BRIDGE()), "initial bridge not whitelisted");
}

#[test]
fn test_consume_records_inference() {
    let d = deploy();

    let inference_id: felt252 = 0xABCD;
    let model_hash: felt252 = 0x1111;
    let timestamp: u64 = 1700000000;
    let submitter: felt252 = 0xDEAD;

    let l1_handler = L1HandlerTrait::new(d.contract_address, selector!("consume_inference_result"));

    l1_handler
        .execute(
            L1_BRIDGE().into(),
            array![inference_id, model_hash, timestamp.into(), submitter].span(),
        )
        .unwrap();

    assert!(d.is_inference_verified(inference_id), "should be verified");

    let record = d.get_inference_record(inference_id);
    assert_eq!(record.inference_id, inference_id);
    assert_eq!(record.model_hash, model_hash);
    assert_eq!(record.submitter, submitter);
    assert_eq!(record.verified_at, timestamp);
}

#[test]
fn test_consume_emits_event() {
    let d = deploy();
    let mut spy = spy_events();

    let inference_id: felt252 = 0xBEEF;
    let model_hash: felt252 = 0x2222;
    let timestamp: u64 = 1700000001;
    let submitter: felt252 = 0xCAFE;

    let l1_handler = L1HandlerTrait::new(d.contract_address, selector!("consume_inference_result"));

    l1_handler
        .execute(
            L1_BRIDGE().into(),
            array![inference_id, model_hash, timestamp.into(), submitter].span(),
        )
        .unwrap();

    spy
        .assert_emitted(
            @array![
                (
                    d.contract_address,
                    InferenceVerifier::Event::InferenceVerified(
                        InferenceVerifier::InferenceVerified {
                            inference_id, model_hash, submitter, verified_at: timestamp,
                        },
                    ),
                ),
            ],
        );
}


#[test]
#[should_panic(expected: "already recorded")]
fn test_replay_rejected() {
    let d = deploy();
    let inference_id: felt252 = 0x9999;

    L1HandlerTrait::new(d.contract_address, selector!("consume_inference_result"))
        .execute(L1_BRIDGE().into(), array![inference_id, 0x1, 1000_u64.into(), 0x1].span())
        .unwrap();

    let result = L1HandlerTrait::new(d.contract_address, selector!("consume_inference_result"))
        .execute(L1_BRIDGE().into(), array![inference_id, 0x1, 1000_u64.into(), 0x1].span());

    match result {
        Result::Ok(_) => panic!("Should have panicked"),
        Result::Err(panic_data) => panic(panic_data),
    }
}

#[test]
#[should_panic(expected: "unauthorized L1 sender")]
fn test_unauthorized_l1_sender_rejected() {
    let d = deploy();
    let unknown_bridge: felt252 = 0xBADBEEF;

    let l1_handler = L1HandlerTrait::new(d.contract_address, selector!("consume_inference_result"));

    let result = l1_handler
        .execute(unknown_bridge, array![0xAAAA, 0x1, 1000_u64.into(), 0x1].span());

    match result {
        Result::Ok(_) => panic!("Should have panicked"),
        Result::Err(panic_data) => panic(panic_data),
    }
}


#[test]
fn test_add_l1_verifier() {
    let d = deploy();

    assert!(!d.is_l1_verifier(SECOND_BRIDGE()), "should not be whitelisted yet");

    start_cheat_caller_address(d.contract_address, OWNER());
    d.add_l1_verifier(SECOND_BRIDGE());
    stop_cheat_caller_address(d.contract_address);

    assert!(d.is_l1_verifier(SECOND_BRIDGE()), "should be whitelisted");
}

#[test]
#[should_panic(expected: "not owner")]
fn test_add_l1_verifier_only_owner() {
    let d = deploy();
    let rando = contract_address_const::<0x9999>();

    start_cheat_caller_address(d.contract_address, rando);
    d.add_l1_verifier(SECOND_BRIDGE());
}

#[test]
fn test_remove_l1_verifier() {
    let d = deploy();

    start_cheat_caller_address(d.contract_address, OWNER());
    d.remove_l1_verifier(L1_BRIDGE());
    stop_cheat_caller_address(d.contract_address);

    assert!(!d.is_l1_verifier(L1_BRIDGE()), "should be removed");
}

#[test]
fn test_second_bridge_can_settle() {
    let d = deploy();

    start_cheat_caller_address(d.contract_address, OWNER());
    d.add_l1_verifier(SECOND_BRIDGE());
    stop_cheat_caller_address(d.contract_address);

    let inference_id: felt252 = 0x7777;

    let l1_handler = L1HandlerTrait::new(d.contract_address, selector!("consume_inference_result"));

    l1_handler
        .execute(SECOND_BRIDGE().into(), array![inference_id, 0x3, 9000_u64.into(), 0x5].span())
        .unwrap();

    assert!(d.is_inference_verified(inference_id), "second bridge should settle");
}

#[test]
fn test_unverified_returns_false() {
    let d = deploy();
    assert!(!d.is_inference_verified(0xDEADBEEF), "unverified should be false");
}

#[test]
fn test_unverified_record_is_zero() {
    let d = deploy();
    let record = d.get_inference_record(0xDEADBEEF);
    assert_eq!(record.inference_id, 0);
}
