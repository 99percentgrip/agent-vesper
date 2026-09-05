
      # SDK methods

        ## open()

        Opens a session. Returns a handle bound to the supplied
 transport. Errors if the transport probe fails.

        ```
let session = client.open(transport).await?;
```

        ## navigate(url)

        Navigates to a URL. Resolves after the document is ready and
 the event channel reports the load event.

        | Parameter | Type | Required |
| --- | --- | --- |
| url | string | yes |

        ## click(target)

        Clicks an indexed element. The index comes from the serialized
 interactable map. Errors if the index is no longer valid.
