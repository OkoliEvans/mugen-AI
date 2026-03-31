// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console2} from "forge-std/Test.sol";
import {InferenceVerifier} from "../src/InferenceVerifier.sol";

/// @dev Stub verifier — returns true or false based on a flag set per test.
contract MockHalo2Verifier {
    bool public shouldPass = true;

    function setPass(bool _pass) external {
        shouldPass = _pass;
    }

    function verifyProof(
        bytes calldata,
        uint256[] calldata
    ) external view returns (bool) {
        return shouldPass;
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

    InferenceVerifier public verifier;
    MockHalo2Verifier public mockHalo2;

    address internal owner = makeAddr("owner");
    address internal settler = makeAddr("settler");
    address internal attacker = makeAddr("attacker");

    bytes32 internal constant MODEL_ID = keccak256("resnet18-v1");
    bytes32 internal constant INPUT_HASH = keccak256("input-data");
    bytes32 internal constant OUTPUT_HASH = keccak256("output-data");

    bytes internal constant DUMMY_PROOF = hex"deadbeef";
    uint256[] internal DUMMY_INSTANCES = [uint256(1), 2, 3];

    function setUp() public {
        mockHalo2 = new MockHalo2Verifier();
        vm.prank(owner);
        verifier = new InferenceVerifier(address(mockHalo2), owner);

        // Whitelist settler
        vm.prank(owner);
        verifier.setSettler(settler, true);
    }

    // -------------------------------------------------------------------------
    // Construction
    // -------------------------------------------------------------------------

    function test_constructor_setsVerifier() public view {
        assertEq(address(verifier.halo2Verifier()), address(mockHalo2));
    }

    function test_constructor_setsOwner() public view {
        assertEq(verifier.owner(), owner);
    }

    function test_constructor_revertsOnZeroVerifier() public {
        vm.expectRevert(InferenceVerifier.ZeroAddress.selector);
        new InferenceVerifier(address(0), owner);
    }

    function test_constructor_revertsOnZeroOwner() public {
        vm.expectRevert(
            abi.encodeWithSignature("OwnableInvalidOwner(address)", address(0))
        );
        new InferenceVerifier(address(mockHalo2), address(0));
    }

    // -------------------------------------------------------------------------
    // submitProof — happy path
    // -------------------------------------------------------------------------

    function test_submitProof_succeeds() public {
        vm.prank(settler);
        vm.expectEmit(true, true, true, true);
        emit InferenceVerified(OUTPUT_HASH, MODEL_ID, settler, block.timestamp);

        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            MODEL_ID,
            INPUT_HASH,
            OUTPUT_HASH
        );

        assertTrue(verifier.isVerified(OUTPUT_HASH));
    }

    function test_submitProof_attestationStoredCorrectly() public {
        vm.prank(settler);
        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            MODEL_ID,
            INPUT_HASH,
            OUTPUT_HASH
        );

        InferenceVerifier.Attestation memory att = verifier.getAttestation(
            OUTPUT_HASH
        );
        assertEq(att.modelId, MODEL_ID);
        assertEq(att.inputHash, INPUT_HASH);
        assertEq(att.settler, settler);
        assertEq(att.timestamp, uint48(block.timestamp));
    }

    // -------------------------------------------------------------------------
    // submitProof — access control
    // -------------------------------------------------------------------------

    function test_submitProof_revertsIfNotSettler() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSelector(
                InferenceVerifier.NotSettler.selector,
                attacker
            )
        );
        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            MODEL_ID,
            INPUT_HASH,
            OUTPUT_HASH
        );
    }

    // -------------------------------------------------------------------------
    // submitProof — duplicate prevention
    // -------------------------------------------------------------------------

    function test_submitProof_revertsOnDuplicate() public {
        vm.prank(settler);
        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            MODEL_ID,
            INPUT_HASH,
            OUTPUT_HASH
        );

        vm.prank(settler);
        vm.expectRevert(
            abi.encodeWithSelector(
                InferenceVerifier.AlreadyVerified.selector,
                OUTPUT_HASH
            )
        );
        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            MODEL_ID,
            INPUT_HASH,
            OUTPUT_HASH
        );
    }

    // -------------------------------------------------------------------------
    // submitProof — invalid proof
    // -------------------------------------------------------------------------

    function test_submitProof_revertsOnInvalidProof() public {
        mockHalo2.setPass(false);

        vm.prank(settler);
        vm.expectRevert(InferenceVerifier.InvalidProof.selector);
        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            MODEL_ID,
            INPUT_HASH,
            OUTPUT_HASH
        );
    }

    // -------------------------------------------------------------------------
    // Pausing
    // -------------------------------------------------------------------------

    function test_pause_blocksSubmission() public {
        vm.prank(owner);
        verifier.pause();

        vm.prank(settler);
        vm.expectRevert(abi.encodeWithSignature("EnforcedPause()"));
        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            MODEL_ID,
            INPUT_HASH,
            OUTPUT_HASH
        );
    }

    function test_unpause_resumesSubmission() public {
        vm.prank(owner);
        verifier.pause();

        vm.prank(owner);
        verifier.unpause();

        vm.prank(settler);
        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            MODEL_ID,
            INPUT_HASH,
            OUTPUT_HASH
        );
        assertTrue(verifier.isVerified(OUTPUT_HASH));
    }

    function test_pause_onlyOwner() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSignature(
                "OwnableUnauthorizedAccount(address)",
                attacker
            )
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
            abi.encodeWithSignature(
                "OwnableUnauthorizedAccount(address)",
                attacker
            )
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
        MockHalo2Verifier newMock = new MockHalo2Verifier();
        vm.prank(owner);
        vm.expectEmit(true, true, false, false);
        emit VerifierUpgraded(address(mockHalo2), address(newMock));

        verifier.upgradeVerifier(address(newMock));
        assertEq(address(verifier.halo2Verifier()), address(newMock));
    }

    function test_upgradeVerifier_onlyOwner() public {
        vm.prank(attacker);
        vm.expectRevert(
            abi.encodeWithSignature(
                "OwnableUnauthorizedAccount(address)",
                attacker
            )
        );
        verifier.upgradeVerifier(address(mockHalo2));
    }

    function test_upgradeVerifier_revertsOnZeroAddress() public {
        vm.prank(owner);
        vm.expectRevert(InferenceVerifier.ZeroAddress.selector);
        verifier.upgradeVerifier(address(0));
    }

    // -------------------------------------------------------------------------
    // Ownership — Ownable2Step
    // -------------------------------------------------------------------------

    function test_ownershipTransfer_requiresAcceptance() public {
        address newOwner = makeAddr("new-owner");

        vm.prank(owner);
        verifier.transferOwnership(newOwner);

        // Still old owner until new owner accepts
        assertEq(verifier.owner(), owner);

        vm.prank(newOwner);
        verifier.acceptOwnership();
        assertEq(verifier.owner(), newOwner);
    }

    // -------------------------------------------------------------------------
    // Fuzz
    // -------------------------------------------------------------------------

    function testFuzz_unverifiedOutputHash_returnsFalse(
        bytes32 randomHash
    ) public view {
        assertFalse(verifier.isVerified(randomHash));
    }

    function testFuzz_submitProof_differentHashes(
        bytes32 modelId,
        bytes32 inputHash,
        bytes32 outputHash
    ) public {
        vm.assume(outputHash != bytes32(0));

        vm.prank(settler);
        verifier.submitProof(
            DUMMY_PROOF,
            DUMMY_INSTANCES,
            modelId,
            inputHash,
            outputHash
        );
        assertTrue(verifier.isVerified(outputHash));
    }
}
