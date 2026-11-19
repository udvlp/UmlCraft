// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf;

public abstract class AstahAPI {
    public static com.change_vision.jude.api.inf.AstahAPI getAstahAPI() throws java.lang.ClassNotFoundException {
        throw new UnsupportedOperationException();
    }
    public abstract com.change_vision.jude.api.inf.project.ProjectAccessor getProjectAccessor();
}
