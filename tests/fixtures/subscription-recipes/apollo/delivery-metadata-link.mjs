import { ApolloLink } from "@apollo/client/core";
import { tap } from "rxjs";

function droppedTotal(result) {
  const value = result?.extensions?.chigiri?.delivery?.droppedTotal;
  if (value === null) {
    return null;
  }
  if (Number.isSafeInteger(value) && value >= 0) {
    return value;
  }
  return undefined;
}

/**
 * Observes Chigiri delivery metadata without becoming a terminating link.
 *
 * Ordinary data, GraphQL errors, sibling extensions, source errors, completion,
 * and cancellation continue through the original Apollo observable.
 */
export function deliveryMetadataLink(onDelivery) {
  if (typeof onDelivery !== "function") {
    throw new TypeError("onDelivery must be a function");
  }

  return new ApolloLink((operation, forward) =>
    forward(operation).pipe(
      tap({
        next(result) {
          const value = droppedTotal(result);
          if (value !== undefined) {
            onDelivery(
              Object.freeze({
                operationName: operation.operationName,
                droppedTotal: value,
              }),
            );
          }
        },
      }),
    ),
  );
}
