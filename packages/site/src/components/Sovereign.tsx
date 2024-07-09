import {bytesToBase64, hexToBytes} from '@metamask/utils';
import React, {useEffect, useState} from 'react';

import {defaultSnapOrigin} from '../config';
import {Button, SignButton, SubmitButton} from './Buttons';

type SovereignState = {
    keyId?: number;
    callMessage: string;
    chainId?: number;
    maxPriorityFee?: number;
    maxFee?: number;
    gasLimit?: number[];
    nonce: number;
    pk?: string;
    tx?: string;
    sequencer?: string;
    status?: string;
};

const trimTrailingSlash = (str: string) => {
    if (str.endsWith('/')) {
        return str.slice(0, -1);
    }
    return str;
}

const isJsonString = (str: string) => {
    try {
        JSON.parse(str);
    } catch (e) {
        return false;
    }
    return true;
}

const parseGasLimit = (gasLimitString: string) => {
    if (gasLimitString.trim() === '') {
        return undefined;
    }

    return gasLimitString
        .split(',')
        .map(item => item.trim())
        .filter(item => item !== '')
        .map(item => parseInt(item, 10))
        .filter(item => !isNaN(item) && item >= 0); // Ensure all values are valid positive integers
};



export const Sovereign = () => {
    const initialState: SovereignState = {
        keyId: 0,
        chainId: 0,
        maxPriorityFee: 0,
        maxFee: 3000,
        gasLimit: undefined,
        nonce: 0,
        callMessage:
            '{"bank":{"CreateToken":{"salt":11,"token_name":"sov-test-token","initial_balance":1000000,"minter_address":"sov15vspj48hpttzyvxu8kzq5klhvaczcpyxn6z6k0hwpwtzs4a6wkvqwr57gc","authorized_minters":["sov1l6n2cku82yfqld30lanm2nfw43n2auc8clw7r5u5m6s7p8jrm4zqrr8r94","sov15vspj48hpttzyvxu8kzq5klhvaczcpyxn6z6k0hwpwtzs4a6wkvqwr57gc"]}}}',
        sequencer: 'http://localhost:12346/sequencer/',
        status: '',
    };

    const [state, setState] = useState(initialState);

    const submitSequencer = async (body: string, endpoint: string) => {
        try {
            if (!state.sequencer) {
                throw new Error('Sequencer URL field is empty');
            }

            // Options for the fetch request
            const options = {
                method: 'POST', // The HTTP method
                headers: {
                    'Content-Type': 'application/json', // Set the content type to JSON
                },
                body: body,
            };

            const response = await fetch(
                trimTrailingSlash(state.sequencer) + "/" + endpoint,
                options,
            );

            // Check if the response is OK (status is in the range 200-299)
            if (!response.ok) {
                throw new Error('Sequencer response was not ok ' + response.statusText);
            }

            // Parse the JSON from the response
            const responseData = await response.json();

            // Handle the response data
            console.log('Success:', responseData);

            setState({
                ...state,
                status: responseData,
            });
        } catch (er) {
            setState({
                ...state,
                status: er.message,
            });
        }
    }

    const getAddress = async () => {
        const keyId = state.keyId;
        const addressRequest = {
            method: 'wallet_invokeSnap',
            params: {
                snapId: defaultSnapOrigin,
                request: {
                    method: 'getAddress',
                    params: {
                        keyId,
                    },
                },
            },
        };

        window.ethereum.request<string>(addressRequest).then((res) => {
            setState({
                ...state,
                address: res,
            })
            console.log(res);
        });
    }
    //
    // useEffect(() => {
    //
    //
    // }, [state.keyId]);

    return (
        <div>
            <div>Key ID:</div>
            <div>
                <input
                    type="number"
                    value={state.keyId}
                    onChange={(ev) => {
                        const {value} = ev.target;

                        // Allow only positive integers (whole numbers greater than or equal to zero)
                        const regex = /^[0-9\b]+$/u; // Allows digits only
                        if (value === '') {
                           return ;
                        }

                        if (value === '' || regex.test(value)) {
                            setState({
                                ...state,
                                keyId: parseInt(value, 10),
                            });
                        }
                    }}
                />
            </div>
            <div>Nonce:</div>
            <div>
                <input
                    type="number"
                    value={state.nonce}
                    onChange={(ev) => {
                        const {value} = ev.target;

                        // Allow only positive integers (whole numbers greater than or equal to zero)
                        const regex = /^[0-9\b]+$/u; // Allows digits only
                        if (value === '' || regex.test(value)) {
                            setState({
                                ...state,
                                nonce: parseInt(value, 10),
                            });
                        }
                    }}
                />
            </div>
            <div>Chain Id:</div>
            <div>
                <input
                    type="number"
                    value={state.chainId}
                    onChange={(ev) => {
                        const {value} = ev.target;

                        // Allow only positive integers (whole numbers greater than or equal to zero)
                        const regex = /^[0-9\b]+$/u; // Allows digits only
                        if (value === '' || regex.test(value)) {
                            setState({
                                ...state,
                                chainId: parseInt(value, 10),
                            });
                        }
                    }}
                />
            </div>
            <div>Max Priority Fee:</div>
            <div>
                <input
                    type="number"
                    value={state.maxPriorityFee}
                    onChange={(ev) => {
                        const {value} = ev.target;

                        // Allow only positive integers (whole numbers greater than or equal to zero)
                        const regex = /^[0-9\b]+$/u; // Allows digits only
                        if (value === '' || regex.test(value)) {
                            setState({
                                ...state,
                                maxPriorityFee: parseInt(value, 10),
                            });
                        }
                    }}
                />
            </div>
            <div>Max Fee:</div>
            <div>
                <input
                    type="number"
                    value={state.maxFee}
                    onChange={(ev) => {
                        const {value} = ev.target;

                        // Allow only positive integers (whole numbers greater than or equal to zero)
                        const regex = /^[0-9\b]+$/u; // Allows digits only
                        if (value === '' || regex.test(value)) {
                            setState({
                                ...state,
                                maxFee: parseInt(value, 10),
                            });
                        }
                    }}
                />
            </div>
            <div>Gas Limit:</div>
            <div>
                <input
                    onChange={(ev) => {
                        const { value } = ev.target;

                        // Update the gasLimit in state as a raw string for the textarea value
                        setState({
                            ...state,
                            gasLimit: parseGasLimit(value),
                        });
                    }}
                />
            </div>
            <div>
                <strong>Parsed Gas Limit:</strong> {state.gasLimit ? state.gasLimit.join(", ") : 'Not Set'}
            </div>
            <div>Call message:</div>
            <div>
        <textarea
            value={state.callMessage}
            onChange={(ev) =>
                setState({
                    ...state,
                    callMessage: ev.target.value,
                })
            }
            placeholder="Call message in JSON..."
            rows={5}
            cols={40}
        />
            </div>
            <div>
                <strong>Is Message Valid Json:</strong> {isJsonString(state.callMessage) ? "Yes" : 'No'}
            </div>
            <div>
                <SignButton
                    onClick={async () => {
                        // setState({
                        //     ...state,
                        //     pk: '',
                        //     tx: '',
                        // });

                        try {
                            let {
                                keyId,
                                nonce,
                                callMessage,
                                chainId,
                                maxPriorityFee,
                                maxFee,
                                gasLimit
                            } = state;
                            const msg = JSON.parse(callMessage);
                            const params = {
                                keyId,
                                transaction: {
                                    "call": msg,
                                    "nonce": nonce,
                                    "chain_id": chainId,
                                    "max_priority_fee_bips": maxPriorityFee,
                                    "max_fee": maxFee,
                                    "gas_limit": gasLimit
                                },
                            };

                            const request = {
                                method: 'wallet_invokeSnap',
                                params: {
                                    snapId: defaultSnapOrigin,
                                    request: {
                                        method: 'signTransaction',
                                        params,
                                    },
                                },
                            };

                            const response = await window.ethereum.request<string>(request);
                            setState({
                                ...state,
                                tx: response ?? '',
                            });
                        } catch (er) {
                            setState({
                                ...state,
                                status: er.message,
                            });
                        }
                    }}
                />
            </div>
            <Button onClick={getAddress}>Get Address</Button>
            <div>Address:</div>
            <div>
                <input type="text" value={state.address} readOnly/>
            </div>
            <div>Transaction:</div>
            <div>
                <input type="text" value={state.tx} readOnly/>
            </div>
            <div>Sequencer:</div>
            <div>
                <input
                    type="text"
                    value={state.sequencer}
                    placeholder="Sequencer address..."
                    onChange={(ev) =>
                        setState({
                            ...state,
                            sequencer: ev.target.value,
                        })
                    }
                />
            </div>
            <div>
                <strong>Submit Tx:</strong>
                <SubmitButton
                    onClick={async () => {
                        if (!state.tx) {
                            throw new Error('Tx field is empty');
                        }

                        const body = JSON.stringify(bytesToBase64(hexToBytes(state.tx)));
                        await submitSequencer(body, "txs");
                    }
                }
                />
                <strong>Submit Batch:</strong>
                <SubmitButton
                    onClick={async () => {
                        if (!state.tx) {
                            throw new Error('Tx field is empty');
                        }

                        const body = JSON.stringify({"transactions": [bytesToBase64(hexToBytes(state.tx))] });
                        await submitSequencer(body, "batches");
                    }
                }
                />
            </div>
            <div>Status:</div>
            <div>
        <textarea
            value={JSON.stringify(state.status)}
            placeholder="Response data..."
            rows={5}
            cols={40}
            readOnly
        />
            </div>
        </div>
    );
};
