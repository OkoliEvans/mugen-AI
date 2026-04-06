// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console2} from "forge-std/Test.sol";
import {InferenceVerifier} from "../src/InferenceVerifier.sol";
import {ISP1Verifier} from "@sp1-contracts/ISP1Verifier.sol";

/// @dev Mock SP1 verifier — reverts or passes based on flag set per test.
contract MockSP1Verifier is ISP1Verifier {
    bool public shouldPass = true;

    function setPass(bool _pass) external {
        shouldPass = _pass;
    }

    function verifyProof(
        bytes32,
        bytes calldata,
        bytes calldata
    ) external view {
        if (!shouldPass) revert("MockSP1Verifier: invalid proof");
    }
}

contract InferenceVerifierTest is Test {
    event InferenceVerified(
        bytes32 indexed outputHash,
        bytes32 indexed modelId,
        address indexed settler,
        uint256 timestamp
    );

    event SettlerUpdated(address indexed settler, bool approved);
    event VerifierUpgraded(
        address indexed oldVerifier,
        address indexed newVerifier
    );

    event VKeyUpdated(
        bytes32 indexed oldVKey,
        bytes32 indexed newVKey
    );

    event ModelRegistered(
        bytes32 indexed modelId,
        string          ipfsCid,
        bytes32 indexed ipfsCidHash,
        bytes32         inputShapeHash,
        address indexed registeredBy
    );

    InferenceVerifier public verifier;
    MockSP1Verifier   public mockSP1;

    address internal owner    = makeAddr("owner");
    address internal settler  = makeAddr("settler");
    address internal attacker = makeAddr("attacker");

    bytes32 internal constant VKEY       = keccak256("inference-vkey");
    bytes32 internal constant MODEL_ID   = keccak256("resnet18-v1");
    bytes32 internal constant INPUT_HASH = keccak256("input-data");
    bytes32 internal constant OUTPUT_HASH = keccak256("output-data");

    // publicValues = abi.encode(modelId, inputHash, outputHash)
    bytes internal PUBLIC_VALUES;
    bytes internal constant DUMMY_PROOF = hex"deadbeef";

    function setUp() public {
        mockSP1 = new MockSP1Verifier();

        vm.prank(owner);
        verifier = new InferenceVerifier(
            address(mockSP1),
            VKEY,
            owner
        );

        vm.prank(owner);
        verifier.setSettler(settler, true);

        PUBLIC_VALUES = abi.encode(MODEL_ID, INPUT_HASH, OUTPUT_HASH);
    }

    // -------------------------------------------------------------------------
    // Construction
    // -------------------------------------------------------------------------

    function test_constructor_setsVerifier() public view {
        assertEq(address(verifier.sp1Verifier()), address(mockSP1));
    }

    function test_constructor_setsVKey() public view {
        assertEq(verifier.inferenceVKey(), VKEY);
    }

    function test_constructor_setsOwner() public view {
        assertEq(verifier.owner(), owner);
    }

    function test_constructor_revertsOnZeroVerifier() public {
        vm.expectRevert(InferenceVerifier.ZeroAddress.selector);
        new InferenceVerifier(address(0), VKEY, owner);
    }

    function test_constructor_revertsOnZeroVKey() public {
        vm.expectRevert(InferenceVerifier.ZeroVKey.selector);
        new InferenceVerifier(address(mockSP1), bytes32(0), owner);
    }

    function test_constructor_revertsOnZeroOwner() public {
        vm.expectRevert(
            abi.encodeWithSignature("OwnableInvalidOwner(address)", address(0))
        );
        new InferenceVerifier(address(mockSP1), VKEY, address(0));
    }

    // -------------------------------------------------------------------------
    // submitProof — happy path
    // -------------------------------------------------------------------------

    function test_submitProof_succeeds() public {
        vm.prank(settler);
        vm.expectEmit(true, true, true, true);
        emit InferenceVerified(OUTPUT_HASH, MODEL_ID, settler, block.timestamp);

        verifier.submitProof(DUMMY_PROOF, PUBLIC_VALUES);

        assertTrue(verifier.isVerified(OUTPUT_HASH));
    }

    function test_submitProof_attestationStoredCorrectly() public {
        vm.prank(settler);
        verifier.submitProof(DUMMY_PROOF, PUBLIC_VALUES);

        InferenceVerifier.Attestation memory att = verifier.getAttestation(OUTPUT_HASH);
        assertEq(att.modelId,   MODEL_ID);
        assertEq(att.inputHash, INPUT_HASH);
        assertEq(att.settler,   settler);
        assertEq(att.timestamp, uint48(block.timestamp));
    }

    // -------------------------------------------------------------------------
    // submitProof — access control
    // -------------------------------------------------------------------------

    function test_submitProof_revertsIfNotSettler() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSelector(InferenceVerifier.NotSettler.selector, attacker)
        );
        verifier.submitProof(DUMMY_PROOF, PUBLIC_VALUES);
    }

    // -------------------------------------------------------------------------
    // submitProof — duplicate prevention
    // -------------------------------------------------------------------------

    function test_submitProof_revertsOnDuplicate() public {
        vm.prank(settler);
        verifier.submitProof(DUMMY_PROOF, PUBLIC_VALUES);

        vm.prank(settler);
        vm.expectRevert(
            abi.encodeWithSelector(InferenceVerifier.AlreadyVerified.selector, OUTPUT_HASH)
        );
        verifier.submitProof(DUMMY_PROOF, PUBLIC_VALUES);
    }

    // -------------------------------------------------------------------------
    // submitProof — invalid proof
    // -------------------------------------------------------------------------

    function test_submitProof_revertsOnInvalidProof() public {
        mockSP1.setPass(false);

        vm.prank(settler);
        vm.expectRevert();
        verifier.submitProof(DUMMY_PROOF, PUBLIC_VALUES);
    }

    // -------------------------------------------------------------------------
    // Model registration
    // -------------------------------------------------------------------------

    function test_registerModel_succeeds() public {
        bytes32 inputShapeHash = keccak256(abi.encode([uint64(1), uint64(4)]));

        vm.prank(owner);
        verifier.registerModel(MODEL_ID, "QmTestCid", inputShapeHash);

        assertTrue(verifier.isRegisteredModel(MODEL_ID));
        InferenceVerifier.Model memory m = verifier.getModel(MODEL_ID);
        assertEq(m.ipfsCidHash,    keccak256(bytes("QmTestCid")));
        assertEq(m.inputShapeHash, inputShapeHash);
        assertEq(m.registeredBy,   owner);
    }

    function test_registerModel_onlyOwner() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSignature("OwnableUnauthorizedAccount(address)", attacker)
        );
        verifier.registerModel(MODEL_ID, "QmTestCid", bytes32(0));
    }

    function test_registerModel_revertsOnDuplicate() public {
        vm.prank(owner);
        verifier.registerModel(MODEL_ID, "QmTestCid", bytes32(0));

        vm.prank(owner);
        vm.expectRevert(
            abi.encodeWithSelector(InferenceVerifier.ModelAlreadyRegistered.selector, MODEL_ID)
        );
        verifier.registerModel(MODEL_ID, "QmTestCid2", bytes32(0));
    }

    function test_registerModel_revertsOnEmptyCid() public {
        vm.prank(owner);
        vm.expectRevert(InferenceVerifier.EmptyString.selector);
        verifier.registerModel(MODEL_ID, "", bytes32(0));
    }

    function test_getModel_revertsIfNotRegistered() public {
        vm.expectRevert(
            abi.encodeWithSelector(InferenceVerifier.ModelNotRegistered.selector, MODEL_ID)
        );
        verifier.getModel(MODEL_ID);
    }

    // -------------------------------------------------------------------------
    // Pausing
    // -------------------------------------------------------------------------

    function test_pause_blocksSubmission() public {
        vm.prank(owner);
        verifier.pause();

        vm.prank(settler);
        vm.expectRevert(abi.encodeWithSignature("EnforcedPause()"));
        verifier.submitProof(DUMMY_PROOF, PUBLIC_VALUES);
    }

    function test_unpause_resumesSubmission() public {
        vm.prank(owner);
        verifier.pause();

        vm.prank(owner);
        verifier.unpause();

        vm.prank(settler);
        verifier.submitProof(DUMMY_PROOF, PUBLIC_VALUES);
        assertTrue(verifier.isVerified(OUTPUT_HASH));
    }

    function test_pause_onlyOwner() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSignature("OwnableUnauthorizedAccount(address)", attacker)
        );
        verifier.pause();
    }

    // -------------------------------------------------------------------------
    // Settler management
    // -------------------------------------------------------------------------

    function test_setSettler_addsSettler() public {
        address newSettler = makeAddr("new-settler");
        vm.prank(owner);
        verifier.setSettler(newSettler, true);
        assertTrue(verifier.isSettler(newSettler));
    }

    function test_setSettler_removesSettler() public {
        vm.prank(owner);
        verifier.setSettler(settler, false);
        assertFalse(verifier.isSettler(settler));
    }

    function test_setSettler_onlyOwner() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSignature("OwnableUnauthorizedAccount(address)", attacker)
        );
        verifier.setSettler(attacker, true);
    }

    function test_setSettler_revertsOnZeroAddress() public {
        vm.prank(owner);
        vm.expectRevert(InferenceVerifier.ZeroAddress.selector);
        verifier.setSettler(address(0), true);
    }

    // -------------------------------------------------------------------------
    // Verifier upgrade
    // -------------------------------------------------------------------------

    function test_upgradeVerifier_updatesAddress() public {
        MockSP1Verifier newMock = new MockSP1Verifier();
        vm.prank(owner);
        vm.expectEmit(true, true, false, false);
        emit VerifierUpgraded(address(mockSP1), address(newMock));

        verifier.upgradeVerifier(address(newMock));
        assertEq(address(verifier.sp1Verifier()), address(newMock));
    }

    function test_upgradeVerifier_onlyOwner() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSignature("OwnableUnauthorizedAccount(address)", attacker)
        );
        verifier.upgradeVerifier(address(mockSP1));
    }

    function test_upgradeVerifier_revertsOnZeroAddress() public {
        vm.prank(owner);
        vm.expectRevert(InferenceVerifier.ZeroAddress.selector);
        verifier.upgradeVerifier(address(0));
    }

    // -------------------------------------------------------------------------
    // VKey update
    // -------------------------------------------------------------------------

    function test_updateVKey_updatesValue() public {
        bytes32 newVKey = keccak256("new-vkey");
        vm.prank(owner);
        vm.expectEmit(true, true, false, false);
        emit VKeyUpdated(VKEY, newVKey);

        verifier.updateVKey(newVKey);
        assertEq(verifier.inferenceVKey(), newVKey);
    }

    function test_updateVKey_onlyOwner() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSignature("OwnableUnauthorizedAccount(address)", attacker)
        );
        verifier.updateVKey(keccak256("new-vkey"));
    }

    function test_updateVKey_revertsOnZeroVKey() public {
        vm.prank(owner);
        vm.expectRevert(InferenceVerifier.ZeroVKey.selector);
        verifier.updateVKey(bytes32(0));
    }

    // -------------------------------------------------------------------------
    // Ownership — Ownable2Step
    // -------------------------------------------------------------------------

    function test_ownershipTransfer_requiresAcceptance() public {
        address newOwner = makeAddr("new-owner");

        vm.prank(owner);
        verifier.transferOwnership(newOwner);
        assertEq(verifier.owner(), owner);

        vm.prank(newOwner);
        verifier.acceptOwnership();
        assertEq(verifier.owner(), newOwner);
    }

    // -------------------------------------------------------------------------
    // Fuzz
    // -------------------------------------------------------------------------

    function testFuzz_unverifiedOutputHash_returnsFalse(bytes32 randomHash) public view {
        assertFalse(verifier.isVerified(randomHash));
    }

    function testFuzz_submitProof_differentHashes(
        bytes32 modelId,
        bytes32 inputHash,
        bytes32 outputHash
    ) public {
        vm.assume(outputHash != bytes32(0));

        bytes memory pv = abi.encode(modelId, inputHash, outputHash);
        vm.prank(settler);
        verifier.submitProof(DUMMY_PROOF, pv);
        assertTrue(verifier.isVerified(outputHash));
    }
}